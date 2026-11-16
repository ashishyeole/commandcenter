import SwiftUI
import ImageIO
import UserNotifications
import SQLite3
import UniformTypeIdentifiers
import Speech
import AVFoundation
import Security
import Darwin

func validRecurrence(_ value: String) -> Bool {
    if ["none","daily","weekly","monthly","yearly"].contains(value) { return true }
    let parts=value.split(separator:":"); guard parts.count==2, let count=Int(parts[1]), count>0 else {return false}
    return (parts[0]=="days" && count<=3650) || (parts[0]=="weeks" && count<=520) || (parts[0]=="months" && count<=120)
}
struct Item: Codable, Identifiable {
    var id = UUID().uuidString.lowercased()
    var kind = "note"
    var title = ""
    var body = ""
    var pinned = false
    var priority = "normal"
    var status = "active"
    var due: String? = nil
    var recurrence = "none"
    var url: String? = nil
    var created_at = ISO8601DateFormatter().string(from: Date())
    var updated_at = ISO8601DateFormatter().string(from: Date())
    var device_id = ""
    var revision = 1
    var deleted_at: String? = nil
}
struct Exchange: Codable {
    var format = "personal-command-center-mobile"
    var schema = 1
    var items: [Item]
}
struct PortableDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.json] }
    var data: Data
    init(data: Data) { self.data = data }
    init(configuration: ReadConfiguration) throws { data = configuration.file.regularFileContents ?? Data() }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper { FileWrapper(regularFileWithContents: data) }
}
struct StoreError: LocalizedError { var message: String; var errorDescription: String? { message } }
@MainActor final class LegacyStore: ObservableObject {
    @Published var items: [Item] = []
    @Published var discoveries: [MobileDiscovery] = []
    @Published var error: String?
    private var db: OpaquePointer?
    let device: String
    init() {
        if let existing = UserDefaults.standard.string(forKey: "deviceID") { device = existing }
        else { device = UUID().uuidString.lowercased(); UserDefaults.standard.set(device, forKey: "deviceID") }
        do {
            let dir = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            let file = dir.appendingPathComponent("command-center.db")
            guard sqlite3_open(file.path, &db) == SQLITE_OK else { throw StoreError(message: "Cannot open local database") }
            try execute("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, payload TEXT NOT NULL); CREATE TABLE IF NOT EXISTS history (id TEXT PRIMARY KEY, entity_id TEXT NOT NULL, payload TEXT NOT NULL); CREATE TABLE IF NOT EXISTS feed_cache (source TEXT PRIMARY KEY, payload TEXT NOT NULL); PRAGMA user_version=1;")
            try FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: file.path)
            try load(); try loadDiscoveries()
        } catch { self.error = error.localizedDescription }
    }
    private func loadDiscoveries() throws {
        let stmt=try statement("SELECT payload FROM feed_cache",[]);defer {sqlite3_finalize(stmt)}
        var result:[MobileDiscovery]=[]
        while sqlite3_step(stmt)==SQLITE_ROW {let raw=String(cString:sqlite3_column_text(stmt,0));result += try JSONDecoder().decode([MobileDiscovery].self,from:Data(raw.utf8))}
        discoveries=result
    }
    func cacheDiscoveries(_ source:String, _ rows:[MobileDiscovery]) throws {
        let payload=String(data:try JSONEncoder().encode(rows),encoding:.utf8)!
        let stmt=try statement("INSERT INTO feed_cache VALUES(?,?) ON CONFLICT(source) DO UPDATE SET payload=excluded.payload",[source,payload]);defer{sqlite3_finalize(stmt)}
        guard sqlite3_step(stmt)==SQLITE_DONE else {throw StoreError(message:"Could not cache discoveries")}
        try loadDiscoveries()
    }
    func migrationItems() throws -> [Item] {
        let stmt=try statement("SELECT payload FROM items",[]);defer{sqlite3_finalize(stmt)};var result:[Item]=[]
        while sqlite3_step(stmt)==SQLITE_ROW {result.append(try JSONDecoder().decode(Item.self,from:Data(String(cString:sqlite3_column_text(stmt,0)).utf8)))}
        return result
    }
    private func validate(_ item: Item) throws {
        guard UUID(uuidString:item.id) != nil, UUID(uuidString:item.device_id) != nil,
            !item.title.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty, item.title.utf8.count <= 500, item.body.utf8.count <= 100000,
            ["note","task","saved"].contains(item.kind), ["active","waiting","someday","done"].contains(item.status), ["normal","important","critical"].contains(item.priority), validRecurrence(item.recurrence), item.revision > 0 else {throw StoreError(message:"Invalid item fields")}
        if let due = item.due { let parser=DateFormatter();parser.locale=Locale(identifier:"en_US_POSIX");parser.dateFormat="yyyy-MM-dd";parser.isLenient=false;guard due.count==10,parser.date(from:due) != nil else {throw StoreError(message:"Use a valid YYYY-MM-DD due date")} }
        guard item.recurrence == "none" || item.due != nil else {throw StoreError(message:"Repeating tasks need a due date")}
        guard item.kind == "task" || (item.due == nil && item.recurrence == "none" && item.status == "active") else {throw StoreError(message:"Only tasks can have task attributes")}
        if let value=item.url {guard let url=URLComponents(string:value),["https","http"].contains(url.scheme ?? ""),url.host != nil,url.user == nil,url.password == nil else {throw StoreError(message:"Use an HTTP(S) URL without credentials")}}
        let iso=ISO8601DateFormatter();iso.formatOptions=[.withInternetDateTime,.withFractionalSeconds]
        let plain=ISO8601DateFormatter()
        for timestamp in [item.created_at,item.updated_at] {guard iso.date(from:timestamp) != nil || plain.date(from:timestamp) != nil else {throw StoreError(message:"Invalid timestamp")}}
    }
    private func execute(_ sql: String) throws {
        guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else { throw StoreError(message: String(cString: sqlite3_errmsg(db))) }
    }
    private func statement(_ sql: String, _ strings: [String]) throws -> OpaquePointer {
        var ptr: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &ptr, nil) == SQLITE_OK, let ptr else { throw StoreError(message: "Could not prepare local write") }
        let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
        for (index, string) in strings.enumerated() { sqlite3_bind_text(ptr, Int32(index+1), string, -1, transient) }
        return ptr
    }
    private func load() throws {
        let stmt = try statement("SELECT payload FROM items", [])
        defer { sqlite3_finalize(stmt) }
        var result: [Item] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            let text = String(cString: sqlite3_column_text(stmt, 0))
            result.append(try JSONDecoder().decode(Item.self, from: Data(text.utf8)))
        }
        items = result.filter { $0.deleted_at == nil }.sorted { $0.pinned != $1.pinned ? $0.pinned : $0.updated_at > $1.updated_at }
    }
    func save(_ value: Item) {
        do {
            var item = value
            try validate(item)
            guard !item.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw StoreError(message: "A title is required") }
            item.updated_at = ISO8601DateFormatter().string(from: Date()); item.device_id = device
            if items.contains(where: {$0.id == item.id}) { item.revision += 1 }
            let payload = String(data: try JSONEncoder().encode(item), encoding: .utf8)!
            try execute("BEGIN IMMEDIATE")
            do {
                let stmt = try statement("INSERT INTO items VALUES(?,?) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", [item.id,payload]); defer { sqlite3_finalize(stmt) }
                guard sqlite3_step(stmt) == SQLITE_DONE else { throw StoreError(message: "Save failed") }
                let log = try statement("INSERT INTO history VALUES(?,?,?)", [UUID().uuidString,item.id,payload]); defer { sqlite3_finalize(log) }
                guard sqlite3_step(log) == SQLITE_DONE else { throw StoreError(message: "History write failed") }
                try execute("COMMIT")
            } catch { try? execute("ROLLBACK"); throw error }
            try load()
        } catch { self.error = error.localizedDescription }
    }
    func complete(_ value: Item) {
        var item = value
        if item.recurrence == "none" { item.status = item.status == "done" ? "active" : "done" }
        else {
            let formatter = DateFormatter(); formatter.calendar = Calendar(identifier: .gregorian); formatter.locale = Locale(identifier: "en_US_POSIX"); formatter.dateFormat = "yyyy-MM-dd"
            guard let due = item.due.flatMap({formatter.date(from:$0)}) else { error = "Choose a due date first"; return }
            let parts=item.recurrence.split(separator:":");let unit=String(parts[0]);let count=parts.count==2 ? Int(parts[1]) ?? 1 : 1
            let component: Calendar.Component = ["monthly","months"].contains(unit) ? .month : unit == "yearly" ? .year : .day
            guard let next = Calendar(identifier: .gregorian).date(byAdding: component, value: ["weekly","weeks"].contains(unit) ? count*7 : count, to: due) else { return }
            item.due = formatter.string(from: next); item.status = "active"
        }
        save(item)
    }
    func export() throws -> Data { let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted,.sortedKeys]; return try encoder.encode(Exchange(items: items)) }
    func importMissing(_ data: Data) throws -> Int {
        guard data.count < 32*1024*1024 else { throw StoreError(message: "Import is too large") }
        let exchange = try JSONDecoder().decode(Exchange.self, from: data)
        guard exchange.schema == 1, exchange.format == "personal-command-center-mobile", exchange.items.count <= 10000 else { throw StoreError(message: "Unsupported exchange format") }
        var seen = Set<String>()
        for i in exchange.items {
            try validate(i)
            guard UUID(uuidString:i.id) != nil, UUID(uuidString:i.device_id) != nil, !i.title.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty, i.title.utf8.count <= 500, i.body.utf8.count <= 100000, ["note","task","saved"].contains(i.kind), ["normal","important","critical"].contains(i.priority), ["active","done","waiting","someday"].contains(i.status), validRecurrence(i.recurrence), seen.insert(i.id).inserted else { throw StoreError(message: "Invalid or duplicate item") }
        }
        try execute("BEGIN IMMEDIATE"); var count = 0
        do {
            for item in exchange.items {
                let payload = String(data: try JSONEncoder().encode(item),encoding:.utf8)!
                let stmt = try statement("INSERT OR IGNORE INTO items VALUES(?,?)",[item.id,payload]); defer { sqlite3_finalize(stmt) }
                guard sqlite3_step(stmt) == SQLITE_DONE else { throw StoreError(message:"Import failed") }
                count += Int(sqlite3_changes(db))
            }
            try execute("COMMIT"); try load(); return count
        } catch {try? execute("ROLLBACK");throw error}
    }
}
@MainActor final class VoiceCapture: ObservableObject {
    @Published var transcript = ""
    @Published var active = false
    @Published var error: String?
    private let engine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    func start() {
        SFSpeechRecognizer.requestAuthorization { permission in
            Task { @MainActor in
                guard permission == .authorized else { self.error = "Speech permission was not granted"; return }
                AVAudioApplication.requestRecordPermission { granted in Task { @MainActor in
                    guard granted else { self.error = "Microphone permission was not granted"; return }
                    self.record()
                }}
            }
        }
    }
    private func record() {
        guard let recognizer = SFSpeechRecognizer(), recognizer.isAvailable, recognizer.supportsOnDeviceRecognition else { error = "On-device speech recognition is unavailable for this language/device"; return }
        do {
            stop(); transcript = ""
            try AVAudioSession.sharedInstance().setCategory(.record, mode:.measurement, options:.duckOthers)
            try AVAudioSession.sharedInstance().setActive(true)
            let req = SFSpeechAudioBufferRecognitionRequest(); req.requiresOnDeviceRecognition = true; req.shouldReportPartialResults = true; request = req
            let input = engine.inputNode; let format = input.outputFormat(forBus:0)
            input.installTap(onBus:0,bufferSize:1024,format:format) { buffer,_ in req.append(buffer) }
            engine.prepare(); try engine.start(); active = true
            task = recognizer.recognitionTask(with:req) { result, failure in Task { @MainActor in
                if let result { self.transcript = result.bestTranscription.formattedString }
                if failure != nil || result?.isFinal == true { self.stop() }
            }}
        } catch { self.error = error.localizedDescription; stop() }
    }
    func stop() { if engine.isRunning { engine.stop(); engine.inputNode.removeTap(onBus:0) }; request?.endAudio(); task?.cancel(); task = nil; request = nil; active = false; try? AVAudioSession.sharedInstance().setActive(false,options:.notifyOthersOnDeactivation) }
}
@main struct CommandCenterApp: App { @UIApplicationDelegateAdaptor(PushAppDelegate.self) var pushDelegate; @StateObject private var store = LocalStore(); var body: some Scene { WindowGroup { HomeView().environmentObject(store) } } }
struct HomeView: View {
    @EnvironmentObject var store: LocalStore
    @Environment(\.scenePhase) private var reminderPhase
    @State private var filter = "all"
    @State private var search = ""
    @State private var editing: Item?
    @State private var exporting = false
    @State private var importing = false
    @State private var document = PortableDocument(data:Data())
    @State private var pending: Data?
    @State private var message: String?
    var body: some View {
        NavigationStack {
            List {
                Section { Text("Independent local companion. Manual JSON exchange is not device sync.").font(.caption).foregroundStyle(.secondary) }
                NavigationLink("Paired devices & project continuity") { MobileSyncView(store:store) }
                NavigationLink("Synced attachments") { MobileAttachments(store:store) }
                NavigationLink("Native push notifications") { NativePushSettings() }
                NavigationLink("Local task reminders") { MobileReminderSettings(store:store) }
                NavigationLink("Encrypted private vault") { MobileVaultView(store:store) }
                NavigationLink("Live discoveries & offline cache") { MobileDiscoveryView(store:store) }
                Picker("Show",selection:$filter) { Text("All").tag("all"); Text("Notes").tag("note"); Text("Tasks").tag("task"); Text("Saved").tag("saved") }.pickerStyle(.segmented)
                ForEach(store.items.filter { (filter == "all" || $0.kind == filter) && (search.isEmpty || ($0.title + $0.body).localizedCaseInsensitiveContains(search)) }) { item in
                    HStack {
                        if item.kind == "task" { Button { store.complete(item) } label: { Image(systemName:item.status == "done" ? "checkmark.circle.fill" : "circle") }.buttonStyle(.borderless) }
                        Button { editing = item } label: { VStack(alignment:.leading,spacing:5) { Text(item.title).font(.headline); Text(item.body).lineLimit(2).font(.caption).foregroundStyle(.secondary); if let due = item.due { Text("\(due) · \(item.recurrence)").font(.caption2) } } }.buttonStyle(.plain)
                        Spacer()
                        Button { var next = item; next.pinned.toggle(); store.save(next) } label: { Image(systemName:item.pinned ? "pin.fill" : "pin") }.buttonStyle(.borderless)
                    }.padding(.vertical,5)
                }
                Section("Data ownership") {
                    Text("Ordinary notes are not an encrypted vault. Do not store credentials or sensitive documents. Export before uninstalling.").font(.caption)
                    Button("Export readable JSON") { do { document = PortableDocument(data:try store.export()); exporting = true } catch { message = error.localizedDescription } }
                    Button("Import JSON (keep existing IDs)") { importing = true }
                    Text("Exports are plaintext. Choose a private location.").font(.caption).foregroundStyle(.secondary)
                }
            }.navigationTitle("Command Center").searchable(text:$search)
            .toolbar { Button { var item = Item(); item.device_id = store.device; editing = item } label: { Image(systemName:"plus") } }
            .sheet(item:$editing) { item in CaptureView(item:item) { if store.save($0) { editing = nil } } }
            .fileExporter(isPresented:$exporting,document:document,contentType:.json,defaultFilename:"command-center-mobile") { result in if case .failure(let error) = result { message = error.localizedDescription } }
            .fileImporter(isPresented:$importing,allowedContentTypes:[.json]) { result in do { let url = try result.get(); let access = url.startAccessingSecurityScopedResource(); defer { if access {url.stopAccessingSecurityScopedResource()} }; pending = try Data(contentsOf:url) } catch { message = error.localizedDescription } }
            .confirmationDialog("Import this exchange? Existing IDs will be preserved.",isPresented:Binding(get:{pending != nil},set:{if !$0 {pending=nil}})) { Button("Import missing items") { if let data=pending { do {message="Imported \(try store.importMissing(data)) items"} catch {message=error.localizedDescription} };pending=nil };Button("Cancel",role:.cancel){pending=nil} }
            .alert("Command Center",isPresented:Binding(get:{message != nil || store.error != nil},set:{if !$0{message=nil;store.error=nil}})) { Button("OK"){message=nil;store.error=nil} } message: { Text(message ?? store.error ?? "") }
        }.tint(Color(red:0.22,green:0.37,blue:0.26)).onChange(of:reminderPhase){_,phase in if phase == .active{store.reload()}}.onReceive(Timer.publish(every:60,on:.main,in:.common).autoconnect()){_ in MobileReminders.coordinate(store)}
    }
}
struct CaptureView: View {
    @State var item: Item
    var save: (Item)->Void
    @Environment(\.dismiss) var dismiss
    @StateObject private var voice = VoiceCapture()
    var body: some View {
        NavigationStack { Form {
            Picker("Type",selection:$item.kind) { Text("Note").tag("note");Text("Task").tag("task");Text("Saved").tag("saved") }
            TextField("Title",text:$item.title)
            TextEditor(text:$item.body).frame(minHeight:160)
            Toggle("Pin to Today",isOn:$item.pinned)
            if item.kind == "task" {
                TextField("Due date YYYY-MM-DD",text:Binding(get:{item.due ?? ""},set:{item.due=$0.isEmpty ? nil:$0})).keyboardType(.numbersAndPunctuation)
                TextField("Custom rule: days:3, weekdays:1,3,5 or after:task-ID",text:$item.recurrence).textInputAutocapitalization(.never)
                Text("For an end date append ;until:YYYY-MM-DD. Waiting-on rules use after: followed by a task ID.").font(.caption)
                Picker("Repeat",selection:$item.recurrence){ForEach(Array(Set(["none","daily","weekly","monthly","yearly",item.recurrence])).sorted(),id:\.self){Text($0).tag($0)}}
            }
            if item.kind == "saved" {TextField("Source URL",text:Binding(get:{item.url ?? ""},set:{item.url=$0.isEmpty ? nil:$0})).keyboardType(.URL).textInputAutocapitalization(.never)}
            Section("On-device voice capture") {
                Button(voice.active ? "Stop recording" : "Start on-device dictation") { if voice.active {voice.stop()} else {voice.start()} }
                if !voice.transcript.isEmpty {Text(voice.transcript);Button("Append transcript to note"){item.body += (item.body.isEmpty ? "":"\n") + voice.transcript;voice.transcript=""}}
                if let error=voice.error {Text(error).foregroundStyle(.red)}
                Text("Microphone and speech permission are requested only when you start. No cloud fallback is used.").font(.caption)
            }
        }.navigationTitle("Quick capture").toolbar {ToolbarItem(placement:.cancellationAction){Button("Cancel"){dismiss()}};ToolbarItem(placement:.confirmationAction){Button("Save"){if item.kind != "task"{item.due=nil;item.recurrence="none";item.status="active"};save(item)}.disabled(item.title.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty || (item.recurrence != "none" && !item.recurrence.hasPrefix("after:") && item.due == nil))}}.onDisappear{voice.stop()}
    }
}
}

struct MobileDiscovery: Codable, Identifiable {
    var id:String;var title:String;var description:String;var url:String;var source:String;var metric:String;var fetched_at:String
}
final class NoFeedRedirects: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(_ session:URLSession,task:URLSessionTask,willPerformHTTPRedirection response:HTTPURLResponse,newRequest request:URLRequest,completionHandler:@escaping (URLRequest?)->Void) {completionHandler(nil)}
}
struct MobileDiscoveryView: View {
    @ObservedObject var store:LocalStore
    @State private var source="github"
    @State private var query="robotics"
    @State private var consent=false
    @State private var busy=false
    @State private var message:String?
    var body:some View {
        List {
            Section("Public sources") {
                Picker("Source",selection:$source){Text("GitHub").tag("github");Text("Hacker News").tag("news")}.onChange(of:source){_,_ in consent=false}
                TextField("Search query",text:$query)
                Toggle("Allow this query to the selected public provider",isOn:$consent)
                Button(busy ? "Refreshing…" : "Refresh and cache"){Task{await refresh()}}.disabled(!consent || busy)
                Text("Only this query is sent. Personal records are never included. Cached results work offline; metrics are source snapshots, not growth rankings.").font(.caption)
                if let message {Text(message).font(.caption)}
            }
            ForEach(store.discoveries.filter{$0.source==source}){item in
                VStack(alignment:.leading,spacing:8){Text(item.title).font(.headline);Text(item.description).font(.caption);Text(item.metric).font(.caption2);Text("Fetched: \(item.fetched_at)").font(.caption2)
                    if let url=URL(string:item.url),["http","https"].contains(url.scheme ?? "") {Link("Open source",destination:url)}
                    Button("Save locally") {
                        if store.items.contains(where:{$0.url==item.url}) {message="Already saved.";return}
                        var record=Item();record.device_id=store.device;record.kind="saved";record.title=String(item.title.prefix(500));record.body="\(item.description)\n\(item.metric)\nFetched: \(item.fetched_at)";record.url=item.url;store.save(record);message=store.error ?? "Saved on this device."
                    }
                }.padding(.vertical,6)
            }
        }.navigationTitle("Discover")
    }
    @MainActor private func refresh() async {
        guard consent, query.utf8.count<=300 else {message="Use a query shorter than 300 bytes";return}
        busy=true;message=nil;defer{busy=false}
        do {
            let selected=source;var components=URLComponents(string:selected=="github" ? "https://api.github.com/search/repositories" : "https://hn.algolia.com/api/v1/search")!
            components.queryItems=selected=="github" ? [URLQueryItem(name:"q",value:query.isEmpty ? "robotics":query),URLQueryItem(name:"sort",value:"stars"),URLQueryItem(name:"per_page",value:"25")] : [URLQueryItem(name:"query",value:query),URLQueryItem(name:"tags",value:"story"),URLQueryItem(name:"hitsPerPage",value:"25")]
            let configuration=URLSessionConfiguration.ephemeral;configuration.timeoutIntervalForRequest=25;configuration.httpCookieStorage=nil;configuration.urlCredentialStorage=nil
            let session=URLSession(configuration:configuration,delegate:NoFeedRedirects(),delegateQueue:nil);defer{session.invalidateAndCancel()}
            var request=URLRequest(url:components.url!);request.setValue("PersonalCommandCenter/0.3",forHTTPHeaderField:"User-Agent")
            let (bytes,response)=try await session.bytes(for:request)
            guard let http=response as? HTTPURLResponse,(200...299).contains(http.statusCode) else {throw StoreError(message:"Provider request failed; retry later")}
            var data=Data();for try await byte in bytes {if data.count>=2*1024*1024 {throw StoreError(message:"Provider response too large")};data.append(byte)}
            let json=try JSONSerialization.jsonObject(with:data) as? [String:Any]
            guard let rows=json?[selected=="github" ? "items":"hits"] as? [[String:Any]] else {throw StoreError(message:"Unexpected provider response")}
            let fetched=ISO8601DateFormatter().string(from:Date())
            let discoveries=rows.prefix(30).compactMap {r -> MobileDiscovery? in
                let title=r[selected=="github" ? "full_name":"title"] as? String ?? ""
                let id=selected=="github" ? String(describing:r["id"] ?? "") : r["objectID"] as? String ?? ""
                let link=selected=="github" ? r["html_url"] as? String ?? "" : "https://news.ycombinator.com/item?id=\(id)"
                guard !title.isEmpty,let url=URLComponents(string:link),["https","http"].contains(url.scheme ?? ""),url.host != nil,url.user==nil,url.password==nil else {return nil}
                return MobileDiscovery(id:"\(selected):\(id)",title:title,description:r["description"] as? String ?? "Public story",url:link,source:selected,metric:selected=="github" ? "\(r["stargazers_count"] ?? 0) stars" : "\(r["points"] ?? 0) points",fetched_at:fetched)
            }
            try store.cacheDiscoveries(selected,discoveries);message="Updated \(discoveries.count) cached discoveries."
        }catch{message=error.localizedDescription}
    }
}

@_silgen_name("pcc_mobile_request") private func pccRequest(_ input:UnsafePointer<CChar>)->UnsafeMutablePointer<CChar>?
@_silgen_name("pcc_mobile_free") private func pccFree(_ output:UnsafeMutablePointer<CChar>)
enum NativePeer {
    static func callJSON(_ raw:String) throws -> Data {
        let pointer=raw.withCString {pccRequest($0)};guard let pointer else {throw StoreError(message:"Native core unavailable")};defer{pccFree(pointer)}
        return Data(String(cString:pointer).utf8)
    }
    static func unwrap(_ data:Data) throws -> [String:Any] {
        guard let envelope=try JSONSerialization.jsonObject(with:data) as? [String:Any] else {throw StoreError(message:"Invalid native result")}
        guard envelope["ok"] as? Bool == true else {throw StoreError(message:envelope["error"] as? String ?? "Native operation failed")}
        return envelope["value"] as? [String:Any] ?? [:]
    }
    static func raw(_ value:[String:Any]) throws -> String {String(data:try JSONSerialization.data(withJSONObject:value),encoding:.utf8)!}
    static func value<T:Encodable>(_ value:T) throws -> Any {try JSONSerialization.jsonObject(with:JSONEncoder().encode(value))}
}
enum PeerSecrets {
    static func read(_ account:String) throws -> String? {
        let query:[String:Any]=[kSecClass as String:kSecClassGenericPassword,kSecAttrService as String:"com.personal.commandcenter.peer",kSecAttrAccount as String:account,kSecReturnData as String:true,kSecMatchLimit as String:kSecMatchLimitOne]
        var result:CFTypeRef?;let status=SecItemCopyMatching(query as CFDictionary,&result)
        if status==errSecItemNotFound{return nil};guard status==errSecSuccess,let data=result as? Data,let text=String(data:data,encoding:.utf8) else {throw StoreError(message:"Keychain access failed (\(status))")};return text
    }
    static func write(_ account:String,_ value:String) throws {
        let query:[String:Any]=[kSecClass as String:kSecClassGenericPassword,kSecAttrService as String:"com.personal.commandcenter.peer",kSecAttrAccount as String:account]
        let data=Data(value.utf8);let status=SecItemUpdate(query as CFDictionary,[kSecValueData as String:data] as CFDictionary)
        if status==errSecItemNotFound {var add=query;add[kSecValueData as String]=data;add[kSecAttrAccessible as String]=kSecAttrAccessibleWhenUnlockedThisDeviceOnly;guard SecItemAdd(add as CFDictionary,nil)==errSecSuccess else {throw StoreError(message:"Cannot save device trust in Keychain")}}
        else if status != errSecSuccess {throw StoreError(message:"Cannot update device trust in Keychain")}
    }
}
struct MobileWorkspace:Codable,Identifiable {
    var id:String;var module:String;var title:String;var summary:String;var status:String;var pinned:Bool;var tags:[String];var links:[String];var fields:[String:String];var created_at:String;var updated_at:String;var device_id:String;var revision:Int;var deleted_at:String?
}
@MainActor final class LocalStore:ObservableObject {
    @Published var items:[Item]=[];@Published var discoveries:[MobileDiscovery]=[];@Published var records:[MobileWorkspace]=[];@Published var conflicts:[[String:Any]]=[];@Published var peerStatus:[[String:Any]]=[];@Published var verification="";@Published var error:String?;@Published var busy=false
    let device:String;let path:String
    init(){
        let legacy=LegacyStore();device=legacy.device
        let directory=FileManager.default.urls(for:.applicationSupportDirectory,in:.userDomainMask)[0];path=directory.appendingPathComponent("command-center-core.db").path
        do{let value=try call("initialize",["legacy_items":NativePeer.value(legacy.migrationItems())]);try apply(value);try FileManager.default.setAttributes([.protectionKey:FileProtectionType.completeUntilFirstUserAuthentication],ofItemAtPath:path)}catch{self.error=error.localizedDescription}
    }
    func call(_ operation:String,_ args:[String:Any]=[:]) throws -> [String:Any] {var req=args;req["operation"]=operation;req["path"]=path;req["device"]=device;return try NativePeer.unwrap(NativePeer.callJSON(NativePeer.raw(req)))}
    private func apply(_ value:[String:Any]) throws {
        guard let values=value["items"] else{return};items=try JSONDecoder().decode([Item].self,from:JSONSerialization.data(withJSONObject:values))
        MobileReminders.reconcile(items)
        MobileReminders.coordinate(self)
        peerStatus=value["peer_status"] as? [[String:Any]] ?? [];records=try JSONDecoder().decode([MobileWorkspace].self,from:JSONSerialization.data(withJSONObject:value["records"] ?? []));conflicts=value["conflicts"] as? [[String:Any]] ?? [];verification=value["verification"] as? String ?? ""
        let feeds=value["feeds"] as? [[String:Any]] ?? [];discoveries=try feeds.flatMap{try JSONDecoder().decode([MobileDiscovery].self,from:JSONSerialization.data(withJSONObject:$0["items"] ?? []))}
    }
    func reload(){do{try apply(call("snapshot"))}catch{self.error=error.localizedDescription}}
    @discardableResult func save(_ item:Item)->Bool {guard !busy else {error="Wait for the current sync to finish";return false};do{try apply(call("save",["item":NativePeer.value(item)]));return true}catch{self.error=error.localizedDescription;return false}}
    func complete(_ item:Item){guard !busy else{return};do{try apply(call("complete",["id":item.id]))}catch{self.error=error.localizedDescription}}
    func export() throws -> Data {try JSONSerialization.data(withJSONObject:call("export"),options:[.prettyPrinted,.sortedKeys])}
    func importMissing(_ data:Data) throws -> Int {guard !busy,data.count<=32*1024*1024 else{throw StoreError(message:"Sync active or import too large")};let value=try call("import",["exchange":JSONSerialization.jsonObject(with:data)]);try apply(value["snapshot"] as? [String:Any] ?? [:]);return value["imported"] as? Int ?? 0}
    func cacheDiscoveries(_ source:String,_ rows:[MobileDiscovery]) throws {try apply(call("cache",["source":source,"items":NativePeer.value(rows)]))}
    func saveWorkspace(_ record:MobileWorkspace){guard !busy else{return};do{try apply(call("save_record",["record":NativePeer.value(record)]))}catch{self.error=error.localizedDescription}}
    func perform(_ operation:String,_ args:[String:Any]) async throws -> [String:Any] {
        guard !busy else{throw StoreError(message:"A peer operation is already active")};busy=true;defer{busy=false}
        var req=args;req["operation"]=operation;req["path"]=path;req["device"]=device;let raw=try NativePeer.raw(req)
        let data=try await Task.detached{try NativePeer.callJSON(raw)}.value;let value=try NativePeer.unwrap(data);try apply(value["snapshot"] as? [String:Any] ?? value);return value
    }
    func identity() throws -> String {if let key=try PeerSecrets.read("identity-\(device)"){return key};let value=try call("keygen");guard let key=value["identity"] as? String else{throw StoreError(message:"Identity generation failed")};try PeerSecrets.write("identity-\(device)",key);return key}
    func trusted() throws -> [[String:Any]] {guard let raw=try PeerSecrets.read("trust-\(device)") else{return []};return try JSONSerialization.jsonObject(with:Data(raw.utf8)) as? [[String:Any]] ?? []}
    func setTrust(_ values:[[String:Any]]) throws {try PeerSecrets.write("trust-\(device)",String(data:JSONSerialization.data(withJSONObject:values),encoding:.utf8)!);_ = try call("listener_refresh",["trusts":values])}
}
struct MobileSyncView:View {
    @Environment(\.scenePhase) private var scenePhase
    @State private var listener:[String:Any]=[:]
    @StateObject private var discovery=LANPeerDiscovery()
    @ObservedObject var store:LocalStore
    @State private var allowMesh=false;@State private var address="";@State private var code="";@State private var label="My iPhone";@State private var message="";@State private var trust:[[String:Any]]=[];@State private var selected:Set<String>=["note","task","saved","projects","missions","sessions"]
    private func stopReceiving(){_ = try? store.call("listener_stop");listener=[:];discovery.stopAdvertising()}
    private func answerPairing(_ pending:[String:Any],_ approved:Bool){do{if approved,let peer=pending["trust"] as? [String:Any]{var next=try store.trusted();next.removeAll{$0["device"] as? String==peer["device"] as? String};next.append(peer);try store.setTrust(next);trust=next};_ = try store.call("listener_approve",["id":pending["id"] as? String ?? "","approved":approved]);listener=try store.call("listener_status")}catch{message=error.localizedDescription}}
    private let scopes=["note","task","saved","projects","missions","sessions","ideas","learning","crm","events","attachment"]
    var body:some View {
        List{
            Section("Foreground encrypted LAN sync"){
                Text("Pair with a desktop on the same private IPv4 network. Receiving is optional and stops when this screen closes or the app leaves the foreground. Continuous background sync is not promised. Keys and trust stay in this device’s Keychain.").font(.caption)
                Toggle("Use my existing private mesh/VPN",isOn:$allowMesh).disabled(store.busy)
                Text("For cross-network use, connect both devices to your own VPN, enable mesh mode on desktop, then enter its virtual IPv4 address and port. No VPN or relay is installed by this app.").font(.caption)
                Button("Find desktops on this LAN"){discovery.start()}
                ForEach(discovery.addresses,id:\.self){candidate in Button("Untrusted candidate: \(candidate)"){address=candidate;discovery.stop()}}
                if let issue=discovery.issue{Text(issue).font(.caption)}
                TextField("Device name",text:$label);TextField("Desktop IP:port",text:$address).textInputAutocapitalization(.never)
                ForEach(scopes,id:\.self){scope in Toggle(scope,isOn:Binding(get:{selected.contains(scope)},set:{if $0{selected.insert(scope)}else{selected.remove(scope)}}))}
                TextField("Desktop pairing code",text:$code,axis:.vertical).textInputAutocapitalization(.never).autocorrectionDisabled()
                Button(store.busy ? "Connecting / awaiting approval…":"Pair with desktop") {Task{do{let result=try await store.perform("pair",["allow_mesh":allowMesh,"address":address,"code":code.trimmingCharacters(in:.whitespacesAndNewlines),"label":label,"scopes":selected.sorted(),"identity":store.identity()]);guard let peer=result["trust"] as? [String:Any],let id=peer["device"] as? String else{throw StoreError(message:"Missing trust result")};var peers=try store.trusted();peers.removeAll{$0["device"] as? String==id};peers.append(peer);try store.setTrust(peers);trust=peers;code="";message="Paired. Sync now to exchange selected records."}catch{message=error.localizedDescription}}}.disabled(store.busy||code.isEmpty||address.isEmpty||selected.isEmpty)
                if store.busy && !store.verification.isEmpty{Text("Compare on desktop: \(store.verification)").font(.headline)}
                if !message.isEmpty{Text(message).font(.caption)}
            }
            Section("Receive on this mobile device"){
                Text("Enable a foreground listener to pair or sync directly with another phone or desktop. Transfer the one-time code privately, then compare and approve. Restart receiving to create a fresh five-minute invitation.").font(.caption)
                if listener["running"] as? Bool == true {
                    Text("Listening on port \(listener["port"] as? Int ?? 0)")
                    if let invitation=listener["code"] as? String{Text(invitation).font(.caption.monospaced()).textSelection(.enabled)}
                    Text(listener["health"] as? String ?? "").font(.caption)
                    Button("Stop receiving"){stopReceiving()}
                }else{
                    Button("Enable receiving & create invitation"){do{listener=try store.call("listener_start",["identity":store.identity(),"trusts":store.trusted(),"label":label,"scopes":selected.sorted(),"allow_mesh":allowMesh]);if let port=listener["port"] as? Int{discovery.advertise(port)}}catch{message=error.localizedDescription}}.disabled(store.busy||selected.isEmpty)
                }
                if let pending=listener["pending"] as? [String:Any],let peer=pending["trust"] as? [String:Any]{
                    Text("Pair with \(peer["label"] as? String ?? "device")?").font(.headline)
                    Text(pending["verification"] as? String ?? "").font(.headline.monospaced())
                    Text("Compare on both devices. Categories: \((peer["scopes"] as? [String] ?? []).joined(separator:", "))").font(.caption)
                    Button("Codes match — save trust & approve"){answerPairing(pending,true)}
                    Button("Decline",role:.destructive){answerPairing(pending,false)}
                }
            }
            ForEach(Array(trust.enumerated()),id:\.offset){_,peer in Section(peer["label"] as? String ?? "Trusted desktop"){
                Text((peer["scopes"] as? [String] ?? []).joined(separator:", ")).font(.caption)
                let progress=store.peerStatus.first{$0["device"] as? String==peer["device"] as? String}
                Text("Last exchange: \(progress?["last_exchange"] as? String ?? "Never") · \(progress?["pending"] as? Int ?? 0) queued journal entries").font(.caption)
                Button("Sync now"){Task{do{let endpoint=address.isEmpty ? peer["endpoint"] as? String ?? "":address;let result=try await store.perform("sync",["allow_mesh":allowMesh,"trust":peer,"address":endpoint,"label":label,"identity":store.identity()]);var updated=peer;updated["endpoint"]=endpoint;trust=trust.map{$0["device"] as? String==peer["device"] as? String ? updated:$0};try store.setTrust(trust);message="Synced \(result["changed"] as? Int ?? 0) incoming changes."}catch{message=error.localizedDescription}}}.disabled(store.busy)
                Button("Revoke this device",role:.destructive){stopReceiving();do{let id=peer["device"] as? String ?? "";trust.removeAll{$0["device"] as? String==id};try store.setTrust(trust);_ = try store.call("forget",["peer":id]);message="Local trust revoked. Revoke this mobile device on the desktop too; already shared data is not erased."}catch{message=error.localizedDescription}}.disabled(store.busy)
            }}
            Section("Project summaries and plans"){
                ForEach(store.records.filter{["projects","missions","sessions"].contains($0.module)}){record in VStack(alignment:.leading){Text(record.title).font(.headline);Text("\(record.module) · \(record.status)").font(.caption);Text(record.summary).font(.caption).lineLimit(8);if let next=record.fields["next_action"]{Text("Next: \(next)")};if record.module=="missions" && ["draft","planned"].contains(record.status){Button("Approve plan for desktop review"){var value=record;value.status="planned";value.fields["mobile_approval"]="Approved by \(store.device) at \(ISO8601DateFormatter().string(from:Date()))";store.saveWorkspace(value);message="Approval saved locally. Sync to deliver it; no tool is automatically executed."}.disabled(store.busy)}}}
            }
            Section("Conflicts: \(store.conflicts.count)"){
                ForEach(Array(store.conflicts.enumerated()),id:\.offset){_,conflict in let local=conflict["local"] as? [String:Any] ?? [:];let incoming=conflict["incoming"] as? [String:Any] ?? [:];VStack(alignment:.leading){Text(local["title"] as? String ?? "Conflicting record").font(.headline);Text("Local: \(local["body"] as? String ?? local["summary"] as? String ?? "")").font(.caption);Text("Incoming: \(incoming["body"] as? String ?? incoming["summary"] as? String ?? "")").font(.caption);ForEach(["local","incoming"],id:\.self){choice in Button("Use \(choice) version"){do{_ = try store.call("resolve",["id":conflict["id"] as? String ?? "","choice":choice,"revision":local["revision"] as? Int ?? 0]);store.reload()}catch{message=error.localizedDescription}}.disabled(store.busy)}}}
            }
        }.navigationTitle("Device continuity").onDisappear{discovery.stop();stopReceiving()}.onChange(of:scenePhase){_,phase in if phase != .active{stopReceiving();discovery.stop()}}.onAppear{do{trust=try store.trusted();store.reload()}catch{message=error.localizedDescription}}
        .task{while !Task.isCancelled{try? await Task.sleep(for:.seconds(2));if Task.isCancelled{break};listener=(try? store.call("listener_status")) ?? [:];if store.busy || listener["running"] as? Bool == true {store.reload()}}}
    }
}
@MainActor final class LANPeerDiscovery:NSObject,ObservableObject,NetServiceBrowserDelegate,NetServiceDelegate {
    @Published var addresses:[String]=[]
    @Published var issue:String?
    private let browser=NetServiceBrowser()
    private var services:[NetService]=[]
    private var advertised:NetService?
    func advertise(_ port:Int){stopAdvertising();let service=NetService(domain:"local.",type:"_pcc-sync._tcp.",name:"pcc-\(UUID().uuidString)",port:Int32(port));advertised=service;service.publish()}
    func stopAdvertising(){advertised?.stop();advertised=nil}
    override init(){super.init();browser.delegate=self}
    func start(){issue=nil;addresses=[];browser.searchForServices(ofType:"_pcc-sync._tcp.",inDomain:"local.")}
    func stop(){browser.stop();services.forEach{$0.stop()};services=[]}
    func netServiceBrowser(_ browser:NetServiceBrowser,didFind service:NetService,moreComing:Bool){guard services.count<64 else{return};services.append(service);service.delegate=self;service.resolve(withTimeout:5)}
    func netServiceDidResolveAddress(_ sender:NetService){
        for data in sender.addresses ?? [] {let host=data.withUnsafeBytes{buffer -> String? in guard let address=buffer.baseAddress?.assumingMemoryBound(to:sockaddr.self),address.pointee.sa_family==sa_family_t(AF_INET) else{return nil};var name=[CChar](repeating:0,count:Int(NI_MAXHOST));guard getnameinfo(address,socklen_t(data.count),&name,socklen_t(name.count),nil,0,NI_NUMERICHOST)==0 else{return nil};return String(cString:name)};if let host{let endpoint="\(host):\(sender.port)";if !addresses.contains(endpoint){addresses.append(endpoint)}}}
    }
    func netServiceBrowser(_ browser:NetServiceBrowser,didNotSearch errorDict:[String:NSNumber]){issue="LAN discovery unavailable. Enter the desktop address manually."}
}

struct PrivateEntry:Codable,Identifiable {
    var id=UUID().uuidString.lowercased();var title="";var category="document";var body="";var amount:Double?;var due:String?;var filename:String?;var file_base64:String?;var updated_at=ISO8601DateFormatter().string(from:Date())
}
struct PrivateBackupDocument:FileDocument {
    static var readableContentTypes:[UTType]{[.data]};var data:Data
    init(data:Data){self.data=data}
    init(configuration:ReadConfiguration)throws{data=configuration.file.regularFileContents ?? Data()}
    func fileWrapper(configuration:WriteConfiguration)throws->FileWrapper{FileWrapper(regularFileWithContents:data)}
}
struct MobileVaultView:View {
    @ObservedObject var store:LocalStore
    @Environment(\.scenePhase) private var phase
    @State private var key="";@State private var entries:[PrivateEntry]=[];@State private var editing:PrivateEntry?;@State private var expected:String?;@State private var message="";@State private var password="";@State private var exporting=false;@State private var importing=false;@State private var backup=PrivateBackupDocument(data:Data());@State private var pending:Data?;@State private var preview=""
    func lock(){key="";entries=[];editing=nil;expected=nil;password="";preview=""}
    func load()throws{let value=try store.call("vault_list",["key":key]);entries=try JSONDecoder().decode([PrivateEntry].self,from:JSONSerialization.data(withJSONObject:value["entries"] ?? []))}
    func unlock(){do{let account="vault-\(store.device)";if let saved=try PeerSecrets.read(account){key=saved}else{guard let generated=try store.call("vault_keygen")["key"] as? String else{throw StoreError(message:"Cannot create vault key")};try PeerSecrets.write(account,generated);key=generated};try load()}catch{lock();message=error.localizedDescription}}
    var body:some View {
        List {
            Section {Text("Device-local AES-GCM private records. The key stays in Keychain; this view locks when backgrounded or closed. Private records are excluded from ordinary search, feeds, AI and peer sync. Keep a password-encrypted backup before uninstalling.").font(.caption)}
            if key.isEmpty {Button("Unlock private vault"){unlock()}}
            else {
                Button("Lock vault"){lock()}
                Button("New private record"){expected=nil;editing=PrivateEntry()}
                ForEach(entries){entry in Button{expected=entry.updated_at;editing=entry}label:{VStack(alignment:.leading){Text(entry.title);Text(entry.category).font(.caption)}}}
                Section("Encrypted portable backup") {
                    SecureField("Backup password (12+ characters)",text:$password)
                    Text("The password protects the exported archive and is required to restore it. This is a separate vault-only backup, compatible with desktop encrypted archives.").font(.caption)
                    Button("Export encrypted vault archive") {
                        do{let value=try store.call("vault_export",["key":key,"password":password]);guard let text=value["archive_base64"] as? String,let bytes=Data(base64Encoded:text) else{throw StoreError(message:"Invalid backup result")};backup=PrivateBackupDocument(data:bytes);exporting=true;password=""}catch{message=error.localizedDescription}
                    }.disabled(password.count<12)
                    Button("Choose encrypted archive to restore"){importing=true}
                    if pending != nil && preview.isEmpty {Button("Preview selected archive with this password"){do{guard let data=pending else{return};let value=try store.call("vault_preview_import",["key":key,"password":password,"archive_base64":data.base64EncodedString()]);preview=String(data:try JSONSerialization.data(withJSONObject:value,options:.prettyPrinted),encoding:.utf8) ?? ""}catch{message=error.localizedDescription}}.disabled(password.isEmpty);Button("Discard selected encrypted archive"){pending=nil;preview=""}}
                    if !preview.isEmpty {Text(preview).font(.caption);Button("Import reviewed private records as safe copies"){do{guard let data=pending else{return};let result=try store.call("vault_import",["key":key,"password":password,"archive_base64":data.base64EncodedString(),"confirmation":"IMPORT PRIVATE COPIES"]);message="Imported \(result["imported"] ?? 0) private records";pending=nil;preview="";password="";try load()}catch{message=error.localizedDescription}};Button("Cancel import"){pending=nil;preview=""}}
                }
            }
            if !message.isEmpty {Text(message).font(.caption)}
        }.navigationTitle("Private vault").privacySensitive()
        .task(id:key){if !key.isEmpty{do{try await Task.sleep(for:.seconds(600));lock();message="Vault session expired after ten minutes."}catch{}}}
        .onDisappear{lock()}.onChange(of:phase){_,p in if p != .active{lock()}}
        .sheet(item:$editing){initial in PrivateEntryEditor(entry:initial,onCancel:{editing=nil},onSave:{entry in do{var args:[String:Any]=["key":key,"entry":try NativePeer.value(entry)];if let expected{args["expected_updated_at"]=expected};_ = try store.call("vault_save",args);editing=nil;try load()}catch{message=error.localizedDescription;editing=nil}})}
        .fileExporter(isPresented:$exporting,document:backup,contentType:.data,defaultFilename:"command-center-private.pacc"){result in backup=PrivateBackupDocument(data:Data());if case .failure(let error)=result{message=error.localizedDescription}else{message="Encrypted archive exported. Keep the password separately."}}
        .fileImporter(isPresented:$importing,allowedContentTypes:[.data]){result in do{let url=try result.get();let access=url.startAccessingSecurityScopedResource();defer{if access{url.stopAccessingSecurityScopedResource()}};let size=try url.resourceValues(forKeys:[.fileSizeKey]).fileSize ?? 0;guard size<=20*1024*1024 else{throw StoreError(message:"Archive exceeds 20 MiB")};let data=try Data(contentsOf:url);guard data.count<=20*1024*1024 else{throw StoreError(message:"Archive exceeds 20 MiB")};pending=data;preview="";message="Encrypted archive selected. Unlock again if the file picker locked the vault, enter the backup password, then preview before importing."}catch{message=error.localizedDescription}}
    }
}
struct PrivateEntryEditor:View {
    @State var entry:PrivateEntry
    @State private var choosingFile=false
    @State private var fileMessage=""
    var onCancel:()->Void
    var onSave:(PrivateEntry)->Void
    var body:some View {
        NavigationStack {
            Form {
                TextField("Title",text:$entry.title)
                Picker("Category",selection:$entry.category) {
                    ForEach(["document","finance","company","subscription"],id:\.self) { Text($0).tag($0) }
                }
                TextEditor(text:$entry.body).frame(minHeight:220)
                Button(entry.file_base64 == nil ? "Choose private file (up to 10 MiB)" : "Replace private file in draft") { choosingFile=true }
                Text("Only the selected file is read. Save encrypts it with this record. No temporary plaintext file or upload is created.").font(.caption)
                if entry.file_base64 != nil {
                    PrivateFilePreview(entry:entry)
                    Button("Remove attachment from draft",role:.destructive) {entry.filename=nil;entry.file_base64=nil}
                }
                if !fileMessage.isEmpty {Text(fileMessage).font(.caption)}
            }.navigationTitle("Private record").toolbar {
                ToolbarItem(placement:.cancellationAction) { Button("Cancel",action:onCancel) }
                ToolbarItem(placement:.confirmationAction) {
                    Button("Save") { onSave(entry) }.disabled(entry.title.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty)
                }
            }
        }.privacySensitive()
        .fileImporter(isPresented:$choosingFile,allowedContentTypes:[.data]) { result in
            do {
                let url=try result.get();let access=url.startAccessingSecurityScopedResource();defer{if access{url.stopAccessingSecurityScopedResource()}}
                let name=url.lastPathComponent
                guard !name.isEmpty,name.utf8.count<=500,!name.unicodeScalars.contains(where:{CharacterSet.controlCharacters.contains($0)}) else{throw StoreError(message:"Invalid attachment filename")}
                guard let stream=InputStream(url:url) else{throw StoreError(message:"Cannot read selected file")}
                stream.open();defer{stream.close()};var bytes=Data();var buffer=[UInt8](repeating:0,count:8192)
                while true {let n=stream.read(&buffer,maxLength:buffer.count);if n==0{break};guard n>0 else{throw stream.streamError ?? StoreError(message:"File read failed")};guard bytes.count+n<=10*1024*1024 else{throw StoreError(message:"Private file exceeds 10 MiB")};bytes.append(contentsOf:buffer.prefix(n))}
                entry.filename=name;entry.file_base64=bytes.base64EncodedString();if entry.title.isEmpty{entry.title=String(name.prefix(100))};fileMessage="Selected \(name). Save to encrypt; Cancel leaves the saved record unchanged."
            }catch{fileMessage=error.localizedDescription}
        }
    }
}
struct PrivateFilePreview:View {
    let entry:PrivateEntry
    private func image(_ bytes:Data)->CGImage? {
        guard let source=CGImageSourceCreateWithData(bytes as CFData,nil),let props=CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [String:Any],let width=props[kCGImagePropertyPixelWidth as String] as? Int,let height=props[kCGImagePropertyPixelHeight as String] as? Int,width>0,height>0,width<=20000,height<=20000,width*height<=20_000_000 else{return nil}
        return CGImageSourceCreateThumbnailAtIndex(source,0,[kCGImageSourceCreateThumbnailFromImageAlways:true,kCGImageSourceThumbnailMaxPixelSize:1600,kCGImageSourceCreateThumbnailWithTransform:true] as CFDictionary)
    }
    var body:some View {
        if let text=entry.file_base64,text.utf8.count<=14*1024*1024,let bytes=Data(base64Encoded:text),bytes.count<=10*1024*1024 {
            let name=entry.filename ?? "Private attachment"
            Text("\(name) · \(bytes.count) bytes").font(.caption)
            let ext=URL(fileURLWithPath:name).pathExtension.lowercased()
            if ["png","jpg","jpeg"].contains(ext),let preview=image(bytes) {Image(decorative:preview,scale:1).resizable().scaledToFit().frame(maxHeight:300)}
            else if ["txt","md","csv","json","log","yaml","yml","xml","rs","py","js","ts","html"].contains(ext) {Text(String(decoding:bytes.prefix(12000),as:UTF8.self)).font(.caption.monospaced());if bytes.count>12000{Text("Preview limited to first 12,000 bytes.").font(.caption)}}
            else {Text("No inline preview for this type or image size. The original is retained for encrypted backup.").font(.caption)}
        }
    }
}

@MainActor enum MobileReminders {
    static let enabledKey="pcc-local-reminders-enabled"
    static let coordinatedKey="pcc-coordinated-reminders"
    private static var coordinating=false
    static func coordinate(_ store:LocalStore) {
        guard UserDefaults.standard.bool(forKey:enabledKey),UserDefaults.standard.bool(forKey:coordinatedKey),!coordinating,UIApplication.shared.applicationState == .active else{return}
        let hour=Calendar.current.component(.hour,from:Date());guard hour>=8 && hour<22 else{return}
        coordinating=true
        Task { @MainActor in
            defer{coordinating=false}
            let center=UNUserNotificationCenter.current();let settings=await center.notificationSettings()
            guard settings.authorizationStatus == .authorized || settings.authorizationStatus == .provisional else{return}
            let formatter=DateFormatter();formatter.dateFormat="yyyy-MM-dd";formatter.locale=Locale(identifier:"en_US_POSIX");let today=formatter.string(from:Date())
            do{let value=try store.call("reminder_candidates",["today":today]);let keys=value["keys"] as? [String] ?? [];guard !keys.isEmpty,UserDefaults.standard.bool(forKey:coordinatedKey) else{return}
                let content=UNMutableNotificationContent();content.title="A task needs your attention";content.body="Open Command Center to review your due tasks.";content.sound = .default
                try await center.add(UNNotificationRequest(identifier:"pcc-coordinated-task-review",content:content,trigger:nil))
                _=try store.call("reminder_submitted",["today":today,"keys":Array(keys.prefix(128)),"consent":true])
            }catch{UserDefaults.standard.set("Coordinated reminder needs retry: \(error.localizedDescription)",forKey:"pcc-reminder-error")}
        }
    }
    private static var scheduling:Task<Void,Never>?
    static func reconcile(_ items:[Item]) {
        scheduling?.cancel()
        scheduling=Task { @MainActor in
            let center=UNUserNotificationCenter.current()
            let requests=await center.pendingNotificationRequests()
            guard !Task.isCancelled else{return}
            center.removePendingNotificationRequests(withIdentifiers:requests.filter{$0.identifier.hasPrefix("pcc-due-")}.map(\.identifier))
            guard UserDefaults.standard.bool(forKey:enabledKey) else{return}
            let now=Date();let calendar=Calendar.current
            let formatter=DateFormatter();formatter.dateFormat="yyyy-MM-dd";formatter.locale=Locale(identifier:"en_US_POSIX")
            let tasks=items.filter{$0.kind=="task" && $0.status=="active" && $0.due != nil && $0.deleted_at==nil}
            for offset in 0..<7 {
                guard !Task.isCancelled else{return}
                guard let day=calendar.date(byAdding:.day,value:offset,to:now),let fire=calendar.date(bySettingHour:9,minute:0,second:0,of:day),fire>now else{continue}
                let date=formatter.string(from:fire)
                guard tasks.contains(where:{($0.due ?? "9999")<=date}) else{continue}
                let content=UNMutableNotificationContent();content.title="Review your due tasks";content.body="Open Command Center to review outstanding work.";content.sound = .default
                let components=calendar.dateComponents([.year,.month,.day,.hour,.minute],from:fire)
                do {try await center.add(UNNotificationRequest(identifier:"pcc-due-\(date)",content:content,trigger:UNCalendarNotificationTrigger(dateMatching:components,repeats:false)))} catch {UserDefaults.standard.set("Notification schedule was rejected by iOS. Review system permission.",forKey:"pcc-reminder-error")}
            }
        }
    }
}
struct MobileReminderSettings:View {
    @ObservedObject var store:LocalStore
    @State private var enabled=UserDefaults.standard.bool(forKey:MobileReminders.enabledKey)
    @State private var message=""
    var body:some View {
        Form {
            Text("Optional generic reminders at 9:00 AM local time for days with outstanding tasks. The next seven days are scheduled from the current local snapshot. Open the app after task edits or sync to refresh them. No task titles appear on the lock screen.")
            Text("System permission, Focus mode, battery settings and device availability affect delivery. Already scheduled alerts can become stale while the app is closed. These generic review reminders do not suppress alerts on other devices.").font(.caption)
            if enabled {Button("Disable & remove scheduled reminders"){UserDefaults.standard.set(false,forKey:MobileReminders.enabledKey);enabled=false;MobileReminders.reconcile([]);message="Scheduled reminders removed."}}
            else {Button("Request permission & enable local reminders"){UNUserNotificationCenter.current().requestAuthorization(options:[.alert,.sound]){granted,error in Task{@MainActor in if let error{message=error.localizedDescription}else if granted{UserDefaults.standard.set(true,forKey:MobileReminders.enabledKey);enabled=true;MobileReminders.reconcile(store.items);message="Upcoming local reminders scheduled."}else{message="Notifications were not allowed. You can change this in iOS Settings."}}}}}
            Toggle("Coordinate foreground task alerts with paired devices",isOn:Binding(get:{UserDefaults.standard.bool(forKey:MobileReminders.coordinatedKey)},set:{value in UserDefaults.standard.set(value,forKey:MobileReminders.coordinatedKey);if value{MobileReminders.coordinate(store)}else{UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers:["pcc-coordinated-task-review"])}})).disabled(!enabled)
            Text("After foreground sync, recent exact-occurrence receipts reduce duplicates. Alerts are generic and respect 10 PM–8 AM quiet hours. Disconnected devices and simultaneous submissions may duplicate; future scheduled digests stay independent. Foreground coordination stops when this app is closed.").font(.caption)
            Button("Refresh scheduled reminders"){MobileReminders.reconcile(store.items);message="Schedule refreshed from current local tasks."}.disabled(!enabled)
            if !message.isEmpty{Text(message)}
        }.navigationTitle("Task reminders")
    }
}

// Remote push is optional and uses separate host credentials, never pairing or vault keys.
final class PushSessionDelegate:NSObject,URLSessionTaskDelegate {
    func urlSession(_ session:URLSession,task:URLSessionTask,willPerformHTTPRedirection response:HTTPURLResponse,newRequest request:URLRequest,completionHandler:@escaping (URLRequest?)->Void){completionHandler(nil)}
}
@MainActor final class NativePush:ObservableObject {
    static let shared=NativePush()
    @Published var status="Native push is disabled until explicitly configured."
    @Published var busy=false
    static let enabled="pcc-native-push-enabled"
    func request(_ config:[String:String],method:String,path:String,body:[String:Any]?=nil) async throws {
        guard let origin=config["origin"],let base=URLComponents(string:origin),base.scheme=="https",base.host != nil,base.user==nil,base.password==nil,base.query==nil,base.fragment==nil,(base.path.isEmpty || base.path=="/"),let url=URL(string:origin.trimmingCharacters(in:CharacterSet(charactersIn:"/"))+path) else {throw StoreError(message:"Use an HTTPS origin without credentials, path or query")}
        var req=URLRequest(url:url);req.httpMethod=method;req.timeoutInterval=25;req.setValue("Bearer "+(config["secret"] ?? ""),forHTTPHeaderField:"Authorization");req.setValue("application/json",forHTTPHeaderField:"Content-Type")
        if let body{req.httpBody=try JSONSerialization.data(withJSONObject:body)}
        let settings=URLSessionConfiguration.ephemeral;settings.httpCookieStorage=nil;settings.urlCache=nil
        let session=URLSession(configuration:settings,delegate:PushSessionDelegate(),delegateQueue:nil);defer{session.invalidateAndCancel()}
        let (stream,response)=try await session.bytes(for:req)
        var size=0;for try await _ in stream {size+=1;if size>65536{throw StoreError(message:"Push host response exceeds limit")}}
        guard let response=response as? HTTPURLResponse,(200..<300).contains(response.statusCode) else{throw StoreError(message:"Push host did not confirm the request. Review configuration and host access.")}
    }
    func config() throws -> [String:String] {guard let raw=try PeerSecrets.read("native-push-config"),let data=raw.data(using:.utf8) else{throw StoreError(message:"Configure push first")};let value=try JSONDecoder().decode([String:String].self,from:data);guard value["id"] != nil,value["secret"] != nil else{throw StoreError(message:"Push is not configured")};return value}
    func register(origin:String,secret:String,environment:String) async {
        busy=true;defer{busy=false}
        do {
            guard secret.count>=32,secret.count<=4096,!secret.contains("\n"),!secret.contains("\r") else{throw StoreError(message:"Use a strong host access token")}
            let old=try? config()
            if let old,let oldOrigin=old["origin"],oldOrigin != origin {throw StoreError(message:"Remove registration from the previous host before changing origins")}
            let values=["origin":origin,"secret":secret,"environment":environment,"id":old?["id"] ?? UUID().uuidString]
            // Validate the supplied origin without contacting it before OS permission.
            guard let u=URLComponents(string:origin),u.scheme=="https",u.host != nil,u.user==nil,u.password==nil,u.query==nil,u.fragment==nil,(u.path.isEmpty || u.path=="/") else{throw StoreError(message:"Use an HTTPS host origin")}
            guard try await UNUserNotificationCenter.current().requestAuthorization(options:[.alert,.sound]) else{throw StoreError(message:"Notification permission was not granted")}
            try PeerSecrets.write("native-push-config",String(data:JSONEncoder().encode(values),encoding:.utf8)!)
            UserDefaults.standard.set(true,forKey:Self.enabled)
            status="Requesting Apple device token; a push-enabled signed device build is required."
            UIApplication.shared.registerForRemoteNotifications()
        } catch {status=error.localizedDescription}
    }
    func received(_ token:Data) {
        guard UserDefaults.standard.bool(forKey:Self.enabled) else{return}
        Task {do{let c=try config();try await request(c,method:"POST",path:"/v1/push/devices",body:["id":c["id"]!,"platform":"apns","environment":c["environment"] ?? "sandbox","token":token.map{String(format:"%02x",$0)}.joined(),"consent":true]);if UserDefaults.standard.bool(forKey:Self.enabled){status="Registered with your host. Provider acceptance does not guarantee delivery."}else{try await request(c,method:"DELETE",path:"/v1/push/devices/"+c["id"]!)}}catch{status="Token registration needs retry: \(error.localizedDescription). Reopen or register again."}}
    }
    func disable() async {
        busy=true;defer{busy=false};UserDefaults.standard.set(false,forKey:Self.enabled);UIApplication.shared.unregisterForRemoteNotifications()
        do{let c=try config();try await request(c,method:"DELETE",path:"/v1/push/devices/"+c["id"]!);try PeerSecrets.write("native-push-config","{}");status="Remote registration removed and local push disabled."}catch{status="Local push disabled. Remote removal unconfirmed; use Remove again to retry. \(error.localizedDescription)"}
    }
}
final class PushAppDelegate:NSObject,UIApplicationDelegate,UNUserNotificationCenterDelegate {
    func application(_ application:UIApplication,didFinishLaunchingWithOptions launchOptions:[UIApplication.LaunchOptionsKey:Any]?=nil)->Bool {UNUserNotificationCenter.current().delegate=self;if UserDefaults.standard.bool(forKey:NativePush.enabled){application.registerForRemoteNotifications()};return true}
    func userNotificationCenter(_ center:UNUserNotificationCenter,willPresent notification:UNNotification,withCompletionHandler completionHandler:@escaping(UNNotificationPresentationOptions)->Void){let remote=notification.request.content.userInfo["pcc_event"] != nil;completionHandler(remote && !UserDefaults.standard.bool(forKey:NativePush.enabled) ? []:[.banner,.sound])}
    func application(_ application:UIApplication,didRegisterForRemoteNotificationsWithDeviceToken deviceToken:Data){NativePush.shared.received(deviceToken)}
    func application(_ application:UIApplication,didFailToRegisterForRemoteNotificationsWithError error:Error){NativePush.shared.status="Apple registration failed. Check signing, push entitlement and network: \(error.localizedDescription)"}
}
struct NativePushSettings:View {
    @ObservedObject var push=NativePush.shared
    @State private var origin=""
    @State private var secret=""
    @State private var environment="sandbox"
    @State private var consent=false
    var body:some View {Form {
        Text("Receive generic task-review alerts directly in this app. Apple and your chosen host receive this device’s push token. No notes, files or task titles are uploaded. Requires your configured host and a signed push-enabled app. Focus, system permission and connectivity affect delivery.")
        TextField("HTTPS push host origin",text:$origin).textInputAutocapitalization(.never).autocorrectionDisabled()
        SecureField("Your host member access token",text:$secret)
        Picker("Apple environment",selection:$environment){Text("Development / sandbox").tag("sandbox");Text("Distribution / production").tag("production")}
        Toggle("Allow token registration and generic alerts",isOn:$consent)
        Button("Register / refresh native push"){Task{await push.register(origin:origin,secret:secret,environment:environment);secret="";consent=false}}.disabled(!consent||push.busy)
        Button("Remove remote registration & disable"){Task{await push.disable()}}.disabled(push.busy)
        Text(push.status).font(.caption)
        Text("Use the device ID below as the desktop native-push recipient. Credentials stay in Keychain and are excluded from exports.").font(.caption)
        Text((try? push.config()["id"]) ?? "Not registered").textSelection(.enabled)
    }.navigationTitle("Native push").onChange(of:origin){_,_ in consent=false}.onChange(of:secret){_,_ in consent=false}.onChange(of:environment){_,_ in consent=false}.onDisappear{secret="";consent=false}}
}


struct MobileAttachments:View {
    @ObservedObject var store:LocalStore
    @State private var rows:[[String:Any]]=[]
    @State private var query=""
    @State private var message=""
    @State private var file=PrivateBackupDocument(data:Data())
    @State private var filename="document"
    @State private var exporting=false
    func load(){do{rows=try store.call("attachments",["query":query])["rows"] as? [[String:Any]] ?? []}catch{message=error.localizedDescription}}
    var body:some View {List {
        Text("Ordinary files received through approved peer sync. They remain on this device offline. Export creates a plaintext copy at a destination you choose; private vault files use the separate vault workflow. Unknown file types are not executed or rendered here.").font(.caption)
        TextField("Search filenames / indexed text",text:$query).onSubmit{load()}
        Button("Refresh attachments"){load()}
        ForEach(rows.indices,id:\.self){index in let row=rows[index];VStack(alignment:.leading){Text(row["filename"] as? String ?? "Document");Text("Owner: \(row["owner_id"] as? String ?? "")").font(.caption);Text("SHA-256: \(row["sha256"] as? String ?? "")").font(.caption2).textSelection(.enabled);Button("Export original file"){do{guard let id=row["id"] as? String else{return};let a=try store.call("attachment",["id":id]);guard let raw=a["content_base64"] as? String,let bytes=Data(base64Encoded:raw) else{throw StoreError(message:"Invalid attachment")};file=PrivateBackupDocument(data:bytes);filename=a["filename"] as? String ?? "document";exporting=true}catch{message=error.localizedDescription}}}}
        if rows.isEmpty{Text("No matching attachments. Pair with both attachment and owner categories enabled, then sync.")}
        Text(message).font(.caption)
    }.navigationTitle("Attachments").onAppear{load()}.fileExporter(isPresented:$exporting,document:file,contentType:.data,defaultFilename:filename){result in file=PrivateBackupDocument(data:Data());switch result{case .success:message="Original file exported. Keep the plaintext copy private.";case .failure(let error):message=error.localizedDescription}}}
}
