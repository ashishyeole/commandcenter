//! Public web-search planning. No API keys, scraping, account access or registration.
use crate::Result;
use chrono::{Datelike, NaiveDate};
use serde_json::{json, Value};
#[tauri::command]
pub fn poker_search_plan(
    location: String,
    terms: String,
    months: u32,
    satellites: bool,
) -> Result<Value> {
    plan(
        &chrono::Local::now().date_naive().to_string(),
        &location,
        &terms,
        months,
        satellites,
    )
}
fn plan(today: &str, location: &str, terms: &str, months: u32, satellites: bool) -> Result<Value> {
    if location.len() > 120 || terms.len() > 200 || !(1..=12).contains(&months) {
        return Err(
            "Use up to 120 location characters, 200 search characters and a 1–12 month horizon"
                .into(),
        );
    }
    let date = NaiveDate::parse_from_str(today, "%Y-%m-%d").map_err(|_| "Invalid search date")?;
    let end = date
        .checked_add_months(chrono::Months::new(months))
        .ok_or("Invalid search horizon")?;
    // Quoting user terms keeps a location/topic from becoming an injected site: filter.
    let phrase = |s: &str| {
        format!(
            "\"{}\"",
            s.chars()
                .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
                .collect::<String>()
        )
    };
    let years = (date.year()..=end.year())
        .map(|y| y.to_string())
        .collect::<Vec<_>>()
        .join(" OR ");
    let query=format!("poker {} schedule (current OR upcoming OR ongoing) ({years}) {} {} -results -recap -winner -highlights",if satellites{"(tournament OR satellite OR qualifier)"}else{"(tournament OR festival OR series)"},if location.trim().is_empty(){String::new()}else{phrase(location.trim())},if terms.trim().is_empty(){String::new()}else{phrase(terms.trim())});
    let sources = [
        ("All organizers and venues", None, None),
        (
            "WSOP / Circuit",
            Some("wsop.com"),
            Some("https://www.wsop.com/schedule/"),
        ),
        (
            "World Poker Tour / Prime",
            Some("worldpokertour.com"),
            Some("https://cms.worldpokertour.com/event/schedule/"),
        ),
        (
            "PokerStars Live / EPT / regional tours",
            Some("pokerstarslive.com"),
            Some("https://www.pokerstarslive.com/"),
        ),
        (
            "Asian Poker Tour",
            Some("theasianpokertour.com"),
            Some("https://www.theasianpokertour.com/"),
        ),
        (
            "RunGood Poker Series",
            Some("rungood.com"),
            Some("https://www.rungood.com/pages/events"),
        ),
    ];
    let searches=sources.iter().map(|(name,site,schedule)|{let q=if let Some(site)=site{format!("{query} site:{site}")}else{query.clone()};let mut google=url::Url::parse("https://www.google.com/search").unwrap();google.query_pairs_mut().append_pair("q",&q);let mut bing=url::Url::parse("https://www.bing.com/search").unwrap();bing.query_pairs_mut().append_pair("q",&q);json!({"name":name,"query":q,"google_url":google.to_string(),"bing_url":bing.to_string(),"schedule_url":schedule})}).collect::<Vec<_>>();
    Ok(
        json!({"from":today,"through":end.to_string(),"searches":searches,"scope":"The first query searches across providers, including local venues and organizers outside this directory. Named shortcuts are not exhaustive. Search engines may return old events; verify event start/end dates on publisher pages. No live results are fetched or schedules inferred by preparing this plan."}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cross_provider_query_rolls_across_year_end_and_encodes_input() {
        let v = plan("2026-11-15", "Las Vegas & India", "PLO", 3, true).unwrap();
        assert_eq!(v["through"], "2027-02-15");
        let rows = v["searches"].as_array().unwrap();
        assert!(!rows[0]["query"].as_str().unwrap().contains("site:"));
        assert!(rows[0]["query"].as_str().unwrap().contains("2026 OR 2027"));
        let u = url::Url::parse(rows[0]["google_url"].as_str().unwrap()).unwrap();
        assert_eq!(
            u.query_pairs().next().unwrap().1,
            rows[0]["query"].as_str().unwrap()
        );
        assert!(rows
            .iter()
            .all(|r| !r["query"].as_str().unwrap().contains("2025")));
    }
    #[test]
    fn invalid_horizons_and_unbounded_input_fail() {
        assert!(plan("2026-11-15", "", "", 0, false).is_err());
        assert!(plan("2026-11-15", "", "", 13, false).is_err());
        assert!(plan("bad-date", "", "", 3, false).is_err());
        assert!(plan("2026-11-15", &"x".repeat(121), "", 3, false).is_err());
    }
}
