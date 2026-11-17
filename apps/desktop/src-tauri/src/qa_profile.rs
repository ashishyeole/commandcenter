//! Isolate installed development-package acceptance tests without rebuilding the app.
//! No arbitrary path, OS home override, normal-profile migration or account setup.
pub(crate) fn identifier(args: &[String], development: bool) -> crate::Result<Option<String>> {
    let positions: Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| a.starts_with("--qa-profile"))
        .collect();
    if positions.is_empty() {
        return Ok(None);
    }
    if !development
        || positions.len() != 1
        || positions[0].1 != "--qa-profile"
        || args.iter().any(|a| a == "--reminder-check")
    {
        return Err("QA profiles require an interactive development build".into());
    }
    let name = args
        .get(positions[0].0 + 1)
        .ok_or("Supply a QA profile name")?;
    if name.is_empty()
        || name.len() > 40
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(
            "QA profile names must contain 1–40 lowercase letters, digits or hyphens".into(),
        );
    }
    Ok(Some(format!("com.personal.commandcenter.qa.{name}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packaged_qa_is_explicit_namespaced_and_rejects_unsafe_or_release_use() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(identifier(&args(&["app"]), true).unwrap(), None);
        let valid = args(&["app", "--qa-profile", "dmg-20261117"]);
        assert_eq!(
            identifier(&valid, true).unwrap().as_deref(),
            Some("com.personal.commandcenter.qa.dmg-20261117")
        );
        assert!(identifier(&valid, false).is_err());
        for input in [
            vec!["--qa-profile"],
            vec!["--qa-profile=normal"],
            vec!["--qa-profile", "../normal"],
            vec!["--qa-profile", ""],
            vec!["--qa-profile", "Main"],
            vec!["--qa-profile", "a", "--qa-profile", "b"],
            vec!["--qa-profile", "a", "--reminder-check"],
        ] {
            assert!(identifier(&args(&input), true).is_err());
        }
    }
}
