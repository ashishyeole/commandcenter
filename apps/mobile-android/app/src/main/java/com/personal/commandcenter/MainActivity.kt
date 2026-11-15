package com.personal.commandcenter

import android.Manifest
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.LocalDate
import java.util.UUID

fun validRecurrence(value:String):Boolean {
    if(value in listOf("none","daily","weekly","monthly","yearly"))return true
    val parts=value.split(":");if(parts.size!=2)return false
    val n=parts[1].toIntOrNull() ?: return false
    return n>0 && when(parts[0]){"days"->n<=3650;"weeks"->n<=520;"months"->n<=120;else->false}
}
class LegacyStore(context: Context) : SQLiteOpenHelper(context, "command-center.db", null, 2) {
    val device: String
    init {
        val preferences = context.getSharedPreferences("device", Context.MODE_PRIVATE)
        device = preferences.getString("id", null) ?: UUID.randomUUID().toString().also { preferences.edit().putString("id", it).apply() }
        setWriteAheadLoggingEnabled(true)
    }
    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE items (id TEXT PRIMARY KEY, payload TEXT NOT NULL)")
        db.execSQL("CREATE TABLE feed_cache (source TEXT PRIMARY KEY, payload TEXT NOT NULL)")
        db.execSQL("CREATE TABLE history (id TEXT PRIMARY KEY, entity_id TEXT NOT NULL, payload TEXT NOT NULL)")
    }
    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {if(oldVersion<2)db.execSQL("CREATE TABLE IF NOT EXISTS feed_cache (source TEXT PRIMARY KEY, payload TEXT NOT NULL)")}
    fun discoveries(source:String):List<JSONObject> {readableDatabase.rawQuery("SELECT payload FROM feed_cache WHERE source=?",arrayOf(source)).use{c->if(!c.moveToFirst())return emptyList();val a=JSONArray(c.getString(0));return (0 until a.length()).map{a.getJSONObject(it)}}}
    fun cacheDiscoveries(source:String,rows:List<JSONObject>){writableDatabase.insertWithOnConflict("feed_cache",null,ContentValues().apply{put("source",source);put("payload",JSONArray(rows).toString())},SQLiteDatabase.CONFLICT_REPLACE).also{require(it!=-1L){"Could not save feed cache"}}}
    fun migrationItems():List<JSONObject>{val result=mutableListOf<JSONObject>();readableDatabase.rawQuery("SELECT payload FROM items",null).use{c->while(c.moveToNext())result+=JSONObject(c.getString(0))};return result}
    fun list(): List<JSONObject> {
        val result = mutableListOf<JSONObject>()
        readableDatabase.rawQuery("SELECT payload FROM items", null).use { cursor -> while(cursor.moveToNext()) result += JSONObject(cursor.getString(0)) }
        return result.filter { it.isNull("deleted_at") }.sortedWith(compareByDescending<JSONObject>{it.optBoolean("pinned")}.thenByDescending{it.optString("updated_at")})
    }
    fun fresh(): JSONObject { val now = Instant.now().toString(); return JSONObject().put("id", UUID.randomUUID().toString()).put("kind","note").put("title","").put("body","").put("pinned",false).put("priority","normal").put("status","active").put("due",JSONObject.NULL).put("recurrence","none").put("url",JSONObject.NULL).put("created_at",now).put("updated_at",now).put("device_id",device).put("revision",1).put("deleted_at",JSONObject.NULL) }
    private fun validate(item: JSONObject) {
        UUID.fromString(item.getString("id")); UUID.fromString(item.getString("device_id"))
        require(item.getString("title").isNotBlank() && item.getString("title").toByteArray().size <= 500 && item.getString("body").toByteArray().size <= 100000) { "Invalid title or content size" }
        require(item.getString("kind") in listOf("note","task","saved"))
        require(item.getString("status") in listOf("active","waiting","someday","done"))
        require(item.getString("priority") in listOf("normal","important","critical"))
        require(validRecurrence(item.getString("recurrence")))
        if(!item.isNull("due")) LocalDate.parse(item.getString("due"))
        require(item.getString("recurrence") == "none" || !item.isNull("due")) { "Repeating tasks need a due date" }
        require(item.getString("kind") == "task" || (item.getString("recurrence") == "none" && item.isNull("due")))
        require(item.getString("kind") == "task" || item.getString("status") == "active")
        if(!item.isNull("url")) {val uri=java.net.URI(item.getString("url"));require(uri.scheme in listOf("http","https") && uri.host != null && uri.userInfo == null) {"Use an HTTP(S) URL without credentials"}}
        if(!item.isNull("deleted_at")) Instant.parse(item.getString("deleted_at"))
        Instant.parse(item.getString("created_at")); Instant.parse(item.getString("updated_at"))
        require(item.getInt("revision") > 0)
    }
    fun save(value: JSONObject) {
        val item = JSONObject(value.toString()); validate(item)
        item.put("updated_at",Instant.now().toString()).put("device_id",device)
        val db = writableDatabase
        db.beginTransaction()
        try {
            db.rawQuery("SELECT payload FROM items WHERE id=?",arrayOf(item.getString("id"))).use { cursor -> if(cursor.moveToFirst()) item.put("revision",JSONObject(cursor.getString(0)).getInt("revision")+1) }
            val values = ContentValues().apply {put("id",item.getString("id"));put("payload",item.toString())}
            db.insertWithOnConflict("items",null,values,SQLiteDatabase.CONFLICT_REPLACE)
            db.insertOrThrow("history",null,ContentValues().apply {put("id",UUID.randomUUID().toString());put("entity_id",item.getString("id"));put("payload",item.toString())})
            db.setTransactionSuccessful()
        } finally {db.endTransaction()}
    }
    fun complete(value: JSONObject) {
        val item = JSONObject(value.toString())
        if(item.getString("recurrence") == "none") item.put("status", if(item.getString("status")=="done") "active" else "done")
        else { val date=LocalDate.parse(item.getString("due"));val next=when(item.getString("recurrence")){"daily"->date.plusDays(1);"weekly"->date.plusWeeks(1);"monthly"->date.plusMonths(1);"yearly"->date.plusYears(1);else->{val parts=item.getString("recurrence").split(":");require(validRecurrence(item.getString("recurrence")));val n=parts[1].toLong();when(parts[0]){"days"->date.plusDays(n);"weeks"->date.plusWeeks(n);else->date.plusMonths(n)}}};item.put("due",next.toString()).put("status","active") }
        save(item)
    }
    fun export(): String = JSONObject().put("format","personal-command-center-mobile").put("schema",1).put("items",JSONArray(list())).toString(2)
    fun importMissing(raw: String): Int {
        require(raw.toByteArray().size <= 32*1024*1024) {"Exchange exceeds 32 MiB"}
        val envelope=JSONObject(raw);require(envelope.getString("format")=="personal-command-center-mobile" && envelope.getInt("schema")==1)
        val items=envelope.getJSONArray("items");require(items.length()<=10000)
        val seen=mutableSetOf<String>()
        for(index in 0 until items.length()){val item=items.getJSONObject(index);validate(item);require(seen.add(item.getString("id"))) {"Duplicate ID"}}
        val db=writableDatabase;var count=0;db.beginTransaction()
        try { for(index in 0 until items.length()){val item=items.getJSONObject(index);val row=db.insertWithOnConflict("items",null,ContentValues().apply{put("id",item.getString("id"));put("payload",item.toString())},SQLiteDatabase.CONFLICT_IGNORE);if(row!=-1L)count++};db.setTransactionSuccessful() } finally {db.endTransaction()}
        return count
    }
}
class MainActivity: ComponentActivity() {
    override fun onStop(){runCatching{MobilePeerNative.request("{\"operation\":\"listener_stop\"}")};super.onStop()}
    override fun onCreate(savedInstanceState: Bundle?) {super.onCreate(savedInstanceState);TaskReminderReceiver.schedule(this);val result=runCatching{LocalStore(this)};setContent{MaterialTheme{val store=result.getOrNull();if(store!=null)Companion(store,this)else Text("Cannot open local data: ${result.exceptionOrNull()?.message}")}}}
}
@OptIn(ExperimentalMaterial3Api::class)
@Composable fun Companion(store: LocalStore,context: Context) {
    var showVault by remember {mutableStateOf(false)}
    var showAttachments by remember {mutableStateOf(false)}
    var showSync by remember {mutableStateOf(false)}
    var showDiscovery by remember {mutableStateOf(false)}
    var records by remember {mutableStateOf(store.list())}
    var query by remember {mutableStateOf("")}
    var filter by remember {mutableStateOf("all")}
    var editing by remember {mutableStateOf<JSONObject?>(null)}
    var message by remember {mutableStateOf<String?>(null)}
    var pending by remember {mutableStateOf<String?>(null)}
    fun refresh(){records=store.list()}
    val export=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")){uri->if(uri!=null){runCatching{context.contentResolver.openOutputStream(uri)?.bufferedWriter()?.use{it.write(store.export())} ?: error("Cannot open destination")}.onSuccess{message="Readable JSON exported. Keep it private."}.onFailure{message=it.message}}}
    val importer=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->if(uri!=null){runCatching{val input=context.contentResolver.openInputStream(uri) ?: error("Cannot open file");input.use { stream -> val out=java.io.ByteArrayOutputStream();val buffer=ByteArray(8192);while(true){val read=stream.read(buffer);if(read<0)break;require(out.size()+read<=32*1024*1024){"Exchange exceeds 32 MiB"};out.write(buffer,0,read)};out.toString("UTF-8") }}.onSuccess{pending=it}.onFailure{message=it.message}}}
    Scaffold(topBar={TopAppBar(title={Text("Command Center")},actions={TextButton(onClick={editing=store.fresh()}){Text("Capture")}})}){padding->
        LazyColumn(Modifier.padding(padding).padding(horizontal=16.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
            item{TextButton(onClick={showSync=true}){Text("Paired devices & project continuity")}}
            item{MobileReminderControls(context)}
            item{NativePushControls(context)}
            item{TextButton(onClick={showAttachments=true}){Text("Synced attachments")}}
            item{TextButton(onClick={showVault=true}){Text("Encrypted private vault")}}
            item{TextButton(onClick={showDiscovery=true}){Text("Live discoveries & offline cache")}}
            item{Text("Independent local companion. Manual exchange is not device sync.",style=MaterialTheme.typography.bodySmall)}
            item{OutlinedTextField(value=query,onValueChange={query=it},label={Text("Search your local memory")},modifier=Modifier.fillMaxWidth())}
            item{Row(horizontalArrangement=Arrangement.spacedBy(6.dp)){listOf("all","note","task","saved").forEach{kind->FilterChip(selected=filter==kind,onClick={filter=kind},label={Text(kind)})}}}
            items(records.filter{(filter=="all"||it.getString("kind")==filter)&&(it.getString("title")+it.getString("body")).contains(query,ignoreCase=true)},key={it.getString("id")}){item->
                Card(modifier=Modifier.fillMaxWidth(),onClick={editing=JSONObject(item.toString())}){Column(Modifier.padding(16.dp)){
                    Row{Text(item.getString("title"),style=MaterialTheme.typography.titleMedium,modifier=Modifier.weight(1f));TextButton(onClick={runCatching{val next=JSONObject(item.toString()).put("pinned",!item.optBoolean("pinned"));store.save(next);refresh()}.onFailure{message=it.message}}){Text(if(item.optBoolean("pinned"))"Unpin" else "Pin")}}
                    Text(item.getString("body"),maxLines=3,style=MaterialTheme.typography.bodySmall)
                    if(!item.isNull("due"))Text("${item.getString("due")} · ${item.getString("recurrence")}",style=MaterialTheme.typography.labelSmall)
                    if(item.getString("kind")=="task")TextButton(onClick={runCatching{store.complete(item);refresh()}.onFailure{message=it.message}}){Text(if(item.getString("status")=="done")"Reopen" else "Complete occurrence")}
                }}
            }
            item{HorizontalDivider();Text("Ordinary notes are not a vault. Do not store credentials or sensitive documents. Export before uninstalling.",style=MaterialTheme.typography.bodySmall);Row{TextButton(onClick={export.launch("command-center-mobile.json")}){Text("Export JSON")};TextButton(onClick={importer.launch(arrayOf("application/json"))}){Text("Import JSON")}}}
        }
    }
    if(showVault)MobileVaultDialog(store,context,onDismiss={showVault=false})
    if(showAttachments)MobileAttachmentsDialog(store,context){showAttachments=false}
    if(showSync)MobileSyncDialog(store,context,onDismiss={showSync=false},onChanged={refresh()})
    if(showDiscovery)MobileDiscoveryDialog(store,context,onDismiss={showDiscovery=false},onSaved={refresh()})
    editing?.let{item->CaptureDialog(item,context,onDismiss={editing=null},onSave={runCatching{store.save(it);refresh();editing=null}.onFailure{message=it.message}})}
    pending?.let{raw->AlertDialog(onDismissRequest={pending=null},title={Text("Import this exchange?")},text={Text("Existing IDs stay unchanged. New records are added in one transaction. No sync is performed.")},confirmButton={TextButton(onClick={runCatching{store.importMissing(raw)}.onSuccess{message="Imported $it records";refresh()}.onFailure{message=it.message};pending=null}){Text("Import")}},dismissButton={TextButton(onClick={pending=null}){Text("Cancel")}})}
    message?.let{text->AlertDialog(onDismissRequest={message=null},title={Text("Command Center")},text={Text(text)},confirmButton={TextButton(onClick={message=null}){Text("OK")}})}
}
@Composable fun CaptureDialog(initial: JSONObject,context:Context,onDismiss:()->Unit,onSave:(JSONObject)->Unit){
    var title by remember{mutableStateOf(initial.getString("title"))};var body by remember{mutableStateOf(initial.getString("body"))};var kind by remember{mutableStateOf(initial.getString("kind"))};var due by remember{mutableStateOf(if(initial.isNull("due"))"" else initial.getString("due"))};var repeat by remember{mutableStateOf(initial.getString("recurrence"))};var url by remember{mutableStateOf(if(initial.isNull("url"))"" else initial.getString("url"))};var recording by remember{mutableStateOf(false)};var error by remember{mutableStateOf("")}
    var recognizer by remember{mutableStateOf<SpeechRecognizer?>(null)}
    fun startVoice(){
        if(!SpeechRecognizer.isOnDeviceRecognitionAvailable(context)){error="On-device speech is unavailable. No cloud fallback is used.";return}
        recognizer?.destroy();recognizer=SpeechRecognizer.createOnDeviceSpeechRecognizer(context).also{speech->speech.setRecognitionListener(object:RecognitionListener{
            override fun onResults(results:Bundle?){val text=results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull();if(text!=null)body+=(if(body.isEmpty())"" else "\n")+text;recording=false}
            override fun onError(code:Int){error="Speech recognition stopped (code $code)";recording=false}
            override fun onReadyForSpeech(params:Bundle?){};override fun onBeginningOfSpeech(){};override fun onRmsChanged(rmsdB:Float){};override fun onBufferReceived(buffer:ByteArray?){};override fun onEndOfSpeech(){recording=false};override fun onPartialResults(partialResults:Bundle?){};override fun onEvent(eventType:Int,params:Bundle?){}
        });speech.startListening(Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL,RecognizerIntent.LANGUAGE_MODEL_FREE_FORM).putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE,true));recording=true}
    }
    val permission=rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()){granted->if(granted)startVoice() else error="Microphone permission was not granted"}
    DisposableEffect(Unit){onDispose{recognizer?.cancel();recognizer?.destroy()}}
    AlertDialog(onDismissRequest=onDismiss,title={Text("Quick capture")},text={LazyColumn(verticalArrangement=Arrangement.spacedBy(10.dp)){
        item{Row{listOf("note","task","saved").forEach{v->FilterChip(selected=kind==v,onClick={kind=v},label={Text(v)})}}}
        item{OutlinedTextField(value=title,onValueChange={title=it},label={Text("Title")})}
        item{OutlinedTextField(value=body,onValueChange={body=it},label={Text("Text / checklist")},minLines=4)}
        if(kind=="task"){item{OutlinedTextField(value=due,onValueChange={due=it},label={Text("Due YYYY-MM-DD")})};item{Column{Text("Repeat schedule");OutlinedTextField(value=repeat,onValueChange={repeat=it},label={Text("Rule: days:3, weekdays:1,3,5, after:task-ID")});Text("Optional end date: append ;until:YYYY-MM-DD",style=MaterialTheme.typography.bodySmall);listOf("none","daily","weekly","monthly","yearly",repeat).distinct().forEach{v->Row{RadioButton(selected=repeat==v,onClick={repeat=v});Text(v)}}}}}
        if(kind=="saved")item{OutlinedTextField(value=url,onValueChange={url=it},label={Text("Source URL")})}
        item{TextButton(onClick={if(recording){recognizer?.stopListening();recording=false}else permission.launch(Manifest.permission.RECORD_AUDIO)}){Text(if(recording)"Stop dictation" else "On-device voice capture")};if(error.isNotEmpty())Text(error,color=MaterialTheme.colorScheme.error)}
    }},confirmButton={TextButton(enabled=title.isNotBlank()&&(kind!="task"||repeat=="none"||repeat.startsWith("after:")||due.isNotBlank()),onClick={val item=JSONObject(initial.toString()).put("title",title).put("body",body).put("kind",kind).put("due",if(kind=="task"&&due.isNotBlank())due else JSONObject.NULL).put("recurrence",if(kind=="task")repeat else "none").put("url",if(kind=="saved"&&url.isNotBlank())url else JSONObject.NULL);if(kind!="task")item.put("status","active");onSave(item)}){Text("Save")}},dismissButton={TextButton(onClick=onDismiss){Text("Cancel")}})
}

@Composable fun MobileDiscoveryDialog(store:LocalStore,context:Context,onDismiss:()->Unit,onSaved:()->Unit){
    var source by remember{mutableStateOf("github")};var query by remember{mutableStateOf("robotics")};var consent by remember{mutableStateOf(false)};var busy by remember{mutableStateOf(false)};var rows by remember{mutableStateOf(store.discoveries(source))};var message by remember{mutableStateOf("")}
    fun refresh(){if(!consent || query.toByteArray().size>300)return;busy=true;val selected=source;val search=query
        Thread{
            val result=runCatching{
                val q=java.net.URLEncoder.encode(search.ifBlank{if(selected=="github")"robotics" else ""},"UTF-8")
                val url=java.net.URL(if(selected=="github")"https://api.github.com/search/repositories?q=$q&sort=stars&per_page=25" else "https://hn.algolia.com/api/v1/search?query=$q&tags=story&hitsPerPage=25")
                val connection=url.openConnection() as javax.net.ssl.HttpsURLConnection
                connection.instanceFollowRedirects=false;connection.connectTimeout=15000;connection.readTimeout=15000;connection.setRequestProperty("User-Agent","PersonalCommandCenter/0.3")
                try{
                    require(connection.responseCode in 200..299){"Provider request failed; retry later"}
                    val raw=connection.inputStream.use{input->val out=java.io.ByteArrayOutputStream();val buffer=ByteArray(8192);while(true){val n=input.read(buffer);if(n<0)break;require(out.size()+n<=2*1024*1024){"Provider response too large"};out.write(buffer,0,n)};out.toString("UTF-8")}
                    val values=JSONObject(raw).getJSONArray(if(selected=="github")"items" else "hits");val fetched=Instant.now().toString()
                    val discoveries=(0 until minOf(values.length(),30)).mapNotNull{index->val r=values.getJSONObject(index);val id=r.optString(if(selected=="github")"id" else "objectID");val title=r.optString(if(selected=="github")"full_name" else "title");val link=if(selected=="github")r.optString("html_url") else "https://news.ycombinator.com/item?id=$id";val uri=runCatching{java.net.URI(link)}.getOrNull();if(title.isBlank()||uri?.scheme !in listOf("http","https")||uri?.host==null||uri.userInfo!=null)null else JSONObject().put("id","$selected:$id").put("title",title).put("description",if(selected=="github")r.optString("description","") else "Public story").put("url",link).put("source",selected).put("metric",if(selected=="github")"${r.optLong("stargazers_count")} stars" else "${r.optLong("points")} points").put("fetched_at",fetched)}
                    store.cacheDiscoveries(selected,discoveries);discoveries
                }finally{connection.disconnect()}
            }
            android.os.Handler(android.os.Looper.getMainLooper()).post{busy=false;result.onSuccess{rows=it;message="Cached ${it.size} discoveries."}.onFailure{message=it.message ?: "Refresh failed"}}
        }.start()
    }
    AlertDialog(onDismissRequest={if(!busy)onDismiss()},title={Text("Discover")},text={LazyColumn(verticalArrangement=Arrangement.spacedBy(10.dp)){
        item{Row{listOf("github","news").forEach{s->FilterChip(selected=source==s,enabled=!busy,onClick={source=s;consent=false;rows=store.discoveries(s)},label={Text(s)})}}}
        item{OutlinedTextField(value=query,onValueChange={query=it},enabled=!busy,label={Text("Public search query")})}
        item{Row{Checkbox(checked=consent,enabled=!busy,onCheckedChange={consent=it});Text("Allow this query to the public provider")};Text("Only the query is sent. Cached metrics are source snapshots, not growth rankings.",style=MaterialTheme.typography.bodySmall);TextButton(enabled=consent&&!busy,onClick={refresh()}){Text(if(busy)"Refreshing…" else "Refresh & cache")};if(message.isNotEmpty())Text(message)}
        items(rows,key={it.getString("id")}){r->Card{Column(Modifier.padding(12.dp)){Text(r.getString("title"),style=MaterialTheme.typography.titleSmall);Text(r.optString("description"),style=MaterialTheme.typography.bodySmall);Text(r.getString("metric"));Text("Fetched: ${r.getString("fetched_at")}",style=MaterialTheme.typography.labelSmall);Row{TextButton(onClick={runCatching{val uri=android.net.Uri.parse(r.getString("url"));require(uri.scheme in listOf("http","https"));context.startActivity(Intent(Intent.ACTION_VIEW,uri))}.onFailure{message=it.message ?: "Could not open source"}}){Text("Source")};TextButton(onClick={runCatching{if(store.list().any{it.optString("url")==r.getString("url")})error("Already saved");store.save(store.fresh().put("kind","saved").put("title",r.getString("title")).put("body","${r.optString("description")}\n${r.getString("metric")}\nFetched: ${r.getString("fetched_at")}").put("url",r.getString("url")));onSaved();message="Saved on this device."}.onFailure{message=it.message ?: "Save failed"}}){Text("Save locally")}}}}}
    }},confirmButton={TextButton(enabled=!busy,onClick=onDismiss){Text("Close")}})
}

object MobilePeerNative {
    init {System.loadLibrary("pcc_mobile_peer")}
    external fun request(input:String):String
}
class LocalStore(context:Context) {
    val device:String
    private val path:String
    @Volatile var syncing=false
    init {
        val legacy=LegacyStore(context);device=legacy.device
        val file=context.getDatabasePath("command-center-core.db");file.parentFile?.mkdirs();path=file.absolutePath
        try {native("initialize",JSONObject().put("legacy_items",JSONArray(legacy.migrationItems())))}finally{legacy.close()}
    }
    fun native(operation:String,args:JSONObject=JSONObject()):JSONObject {
        val request=JSONObject(args.toString()).put("operation",operation).put("path",path).put("device",device)
        val envelope=JSONObject(MobilePeerNative.request(request.toString()));require(envelope.optBoolean("ok")){envelope.optString("error","Native operation failed")};return envelope.getJSONObject("value")
    }
    fun snapshot():JSONObject=native("snapshot")
    fun list():List<JSONObject>{val a=snapshot().getJSONArray("items");return (0 until a.length()).map{a.getJSONObject(it)}}
    fun fresh():JSONObject {val now=Instant.now().toString();return JSONObject().put("id",UUID.randomUUID().toString()).put("kind","note").put("title","").put("body","").put("pinned",false).put("priority","normal").put("status","active").put("due",JSONObject.NULL).put("recurrence","none").put("url",JSONObject.NULL).put("created_at",now).put("updated_at",now).put("device_id",device).put("revision",1).put("deleted_at",JSONObject.NULL)}
    fun save(item:JSONObject){check(!syncing){"Wait for active sync"};native("save",JSONObject().put("item",item))}
    fun complete(item:JSONObject){check(!syncing){"Wait for active sync"};native("complete",JSONObject().put("id",item.getString("id")))}
    fun export():String=native("export").toString(2)
    fun importMissing(raw:String):Int{check(!syncing){"Wait for active sync"};require(raw.toByteArray().size<=32*1024*1024){"Exchange too large"};return native("import",JSONObject().put("exchange",JSONObject(raw))).getInt("imported")}
    fun discoveries(source:String):List<JSONObject>{val feeds=snapshot().getJSONArray("feeds");for(i in 0 until feeds.length()){val feed=feeds.getJSONObject(i);if(feed.getString("source")==source){val items=feed.getJSONArray("items");return (0 until items.length()).map{items.getJSONObject(it)}}};return emptyList()}
    fun cacheDiscoveries(source:String,rows:List<JSONObject>){native("cache",JSONObject().put("source",source).put("items",JSONArray(rows)))}
}
class PeerSecrets(private val context:Context,private val device:String){
    private val prefs=context.getSharedPreferences("peer-encrypted",Context.MODE_PRIVATE)
    private fun key():javax.crypto.SecretKey{
        val store=java.security.KeyStore.getInstance("AndroidKeyStore").apply{load(null)};val alias="pcc-peer-$device"
        (store.getKey(alias,null) as? javax.crypto.SecretKey)?.let{return it}
        val generator=javax.crypto.KeyGenerator.getInstance(android.security.keystore.KeyProperties.KEY_ALGORITHM_AES,"AndroidKeyStore")
        generator.init(android.security.keystore.KeyGenParameterSpec.Builder(alias,android.security.keystore.KeyProperties.PURPOSE_ENCRYPT or android.security.keystore.KeyProperties.PURPOSE_DECRYPT).setBlockModes(android.security.keystore.KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(android.security.keystore.KeyProperties.ENCRYPTION_PADDING_NONE).setKeySize(256).build());return generator.generateKey()
    }
    private fun get(name:String):String?{val encoded=prefs.getString("$device:$name",null)?:return null;val bytes=android.util.Base64.decode(encoded,android.util.Base64.NO_WRAP);require(bytes.size>=28);val cipher=javax.crypto.Cipher.getInstance("AES/GCM/NoPadding");cipher.init(javax.crypto.Cipher.DECRYPT_MODE,key(),javax.crypto.spec.GCMParameterSpec(128,bytes.copyOfRange(0,12)));return String(cipher.doFinal(bytes.copyOfRange(12,bytes.size)),Charsets.UTF_8)}
    private fun put(name:String,value:String){val cipher=javax.crypto.Cipher.getInstance("AES/GCM/NoPadding");cipher.init(javax.crypto.Cipher.ENCRYPT_MODE,key());val bytes=cipher.iv+cipher.doFinal(value.toByteArray());check(prefs.edit().putString("$device:$name",android.util.Base64.encodeToString(bytes,android.util.Base64.NO_WRAP)).commit()){"Could not save encrypted device trust"}}
    fun pushConfig():String?=get("push-config")
    fun savePushConfig(value:String){put("push-config",value)}
    fun identity():String{get("identity")?.let{return it};val result=JSONObject(MobilePeerNative.request(JSONObject().put("operation","keygen").toString()));require(result.optBoolean("ok")){"Could not generate peer identity"};val key=result.getJSONObject("value").getString("identity");put("identity",key);return key}
    fun vaultKey(store:LocalStore):String{get("vault-key")?.let{return it};val key=store.native("vault_keygen").getString("key");put("vault-key",key);return key}
    fun trusts():List<JSONObject>{val a=JSONArray(get("trust")?:"[]");return (0 until a.length()).map{a.getJSONObject(it)}}
    fun saveTrusts(peers:List<JSONObject>){put("trust",JSONArray(peers).toString());val response=JSONObject(MobilePeerNative.request(JSONObject().put("operation","listener_refresh").put("trusts",JSONArray(peers)).toString()));check(response.optBoolean("ok")){response.optString("error","Listener trust update failed")}}
}
@Composable fun MobileSyncDialog(store:LocalStore,context:Context,onDismiss:()->Unit,onChanged:()->Unit){
    var listenerState by remember{mutableStateOf(JSONObject())}
    var allowMesh by remember{mutableStateOf(false)}
    var discovering by remember{mutableStateOf(false)};var candidates by remember{mutableStateOf(listOf<String>())}
    val secrets=remember{PeerSecrets(context,store.device)}
    var label by remember{mutableStateOf("My Android")};var address by remember{mutableStateOf("")};var code by remember{mutableStateOf("")};var busy by remember{mutableStateOf(false)};var message by remember{mutableStateOf("")};var verification by remember{mutableStateOf("")};var peers by remember{mutableStateOf(runCatching{secrets.trusts()}.getOrDefault(emptyList()))};var snapshot by remember{mutableStateOf(store.snapshot())};var selected by remember{mutableStateOf(setOf("note","task","saved","projects","missions","sessions"))}
    DisposableEffect(discovering){
        val manager=context.getSystemService(Context.NSD_SERVICE) as android.net.nsd.NsdManager
        val handler=android.os.Handler(android.os.Looper.getMainLooper())
        val listener=object:android.net.nsd.NsdManager.DiscoveryListener{
            override fun onDiscoveryStarted(type:String){}
            override fun onDiscoveryStopped(type:String){}
            override fun onStartDiscoveryFailed(type:String,error:Int){handler.post{message="LAN discovery unavailable; enter the address manually.";discovering=false}}
            override fun onStopDiscoveryFailed(type:String,error:Int){}
            override fun onServiceLost(service:android.net.nsd.NsdServiceInfo){}
            override fun onServiceFound(service:android.net.nsd.NsdServiceInfo){if(candidates.size>=64)return;manager.resolveService(service,object:android.net.nsd.NsdManager.ResolveListener{
                override fun onResolveFailed(info:android.net.nsd.NsdServiceInfo,error:Int){}
                override fun onServiceResolved(info:android.net.nsd.NsdServiceInfo){val host=info.host?.hostAddress?:return;if(host.contains(":"))return;val endpoint="$host:${info.port}";handler.post{candidates=(candidates+endpoint).distinct().take(64)}}
            })}
        }
        if(discovering)runCatching{manager.discoverServices("_pcc-sync._tcp.",android.net.nsd.NsdManager.PROTOCOL_DNS_SD,listener)}.onFailure{message=it.message?:"Discovery failed"}
        onDispose{if(discovering)runCatching{manager.stopServiceDiscovery(listener)}}
    }
    fun stopReceiving(){runCatching{store.native("listener_stop")};listenerState=JSONObject()}
    DisposableEffect(Unit){val handler=android.os.Handler(android.os.Looper.getMainLooper());val poll=object:Runnable{override fun run(){runCatching{store.native("listener_status")}.onSuccess{listenerState=it;if(it.optBoolean("running")){snapshot=store.snapshot();onChanged()}};handler.postDelayed(this,2000)}};handler.post(poll);onDispose{handler.removeCallbacks(poll);stopReceiving()}}
    val receivingPort=if(listenerState.optBoolean("running"))listenerState.optInt("port")else 0
    DisposableEffect(receivingPort){val manager=context.getSystemService(Context.NSD_SERVICE) as android.net.nsd.NsdManager;val registration=object:android.net.nsd.NsdManager.RegistrationListener{
        override fun onServiceRegistered(info:android.net.nsd.NsdServiceInfo){}
        override fun onRegistrationFailed(info:android.net.nsd.NsdServiceInfo,error:Int){message="Advertising failed; enter this phone’s LAN IP and port manually"}
        override fun onServiceUnregistered(info:android.net.nsd.NsdServiceInfo){}
        override fun onUnregistrationFailed(info:android.net.nsd.NsdServiceInfo,error:Int){}
    };if(receivingPort>0)runCatching{val info=android.net.nsd.NsdServiceInfo().apply{serviceName="pcc-${UUID.randomUUID()}";serviceType="_pcc-sync._tcp.";port=receivingPort};manager.registerService(info,android.net.nsd.NsdManager.PROTOCOL_DNS_SD,registration)}.onFailure{message=it.message?:"Advertising failed"};onDispose{if(receivingPort>0)runCatching{manager.unregisterService(registration)}}}
    fun network(operation:String,args:JSONObject,onDone:(JSONObject)->Unit){busy=true;store.syncing=true;verification="";Thread{val result=runCatching{store.native(operation,args.put("allow_mesh",allowMesh).put("identity",secrets.identity()).put("label",label))};android.os.Handler(android.os.Looper.getMainLooper()).post{busy=false;store.syncing=false;result.onSuccess{runCatching{onDone(it);snapshot=store.snapshot();onChanged()}.onFailure{message=it.message?:"Could not save result"}}.onFailure{message=it.message?:"Peer operation failed"}}}.start()}
    DisposableEffect(busy){val handler=android.os.Handler(android.os.Looper.getMainLooper());val poll=object:Runnable{override fun run(){if(busy){runCatching{store.snapshot().optString("verification","")}.onSuccess{verification=it};handler.postDelayed(this,3000)}}};if(busy)handler.post(poll);onDispose{handler.removeCallbacks(poll)}}
    AlertDialog(onDismissRequest={if(!busy)onDismiss()},title={Text("Device continuity")},text={LazyColumn(verticalArrangement=Arrangement.spacedBy(10.dp)){
        item{Text("Foreground encrypted LAN sync. Pair with your desktop; keys and trust use Android Keystore protection. Receiving is optional and stops on leaving this screen or the app. Continuous background sync is not promised.",style=MaterialTheme.typography.bodySmall)}
        item{Row{Checkbox(checked=allowMesh,enabled=!busy,onCheckedChange={allowMesh=it});Text("Use my existing private mesh/VPN")};Text("Connect both devices to your own VPN; enable mesh on desktop, then enter its virtual IPv4 address and port. No VPN or relay is installed by this app.",style=MaterialTheme.typography.bodySmall)}
        item{TextButton(enabled=!busy,onClick={discovering=!discovering}){Text(if(discovering)"Stop LAN discovery" else "Find nearby desktops")};candidates.forEach{candidate->TextButton(enabled=!busy,onClick={address=candidate;discovering=false}){Text("Untrusted candidate: $candidate")}}}
        item{OutlinedTextField(value=label,onValueChange={label=it},enabled=!busy,label={Text("Device name")})}
        item{OutlinedTextField(value=address,onValueChange={address=it},enabled=!busy,label={Text("Inviting device IPv4 address:port")})}
        item{Column{listOf("note","task","saved","projects","missions","sessions","ideas","learning","crm","events","attachment").forEach{s->Row{Checkbox(checked=s in selected,enabled=!busy,onCheckedChange={selected=if(it)selected+s else selected-s});Text(s)}}}}
        item{OutlinedTextField(value=code,onValueChange={code=it},enabled=!busy,label={Text("Inviting device pairing code")},minLines=3)}
        item{TextButton(enabled=!busy&&address.isNotBlank()&&code.isNotBlank()&&selected.isNotEmpty(),onClick={network("pair",JSONObject().put("address",address).put("code",code.trim()).put("scopes",JSONArray(selected.sorted()))){result->val peer=result.getJSONObject("trust");peers=peers.filter{it.getString("device")!=peer.getString("device")}+peer;secrets.saveTrusts(peers);code="";message="Paired. Sync now to exchange selected data."}}){Text(if(busy)"Connecting / awaiting approval…" else "Pair with device")};if(busy&&verification.isNotBlank())Text("Compare on desktop: $verification",style=MaterialTheme.typography.titleMedium);if(message.isNotEmpty())Text(message)}
        item{Text("Receive on this phone",style=MaterialTheme.typography.titleMedium);Text("Enable receiving to pair or sync with another phone or desktop. Transfer the one-time code privately; compare and approve. Restart for a fresh five-minute code.",style=MaterialTheme.typography.bodySmall)
            if(listenerState.optBoolean("running")){Text("Listening on port ${listenerState.optInt("port")}");if(!listenerState.isNull("code"))androidx.compose.foundation.text.selection.SelectionContainer{Text(listenerState.optString("code"),style=MaterialTheme.typography.bodySmall)};Text(listenerState.optString("health"));TextButton(onClick={stopReceiving()}){Text("Stop receiving")}}
            else TextButton(enabled=!busy&&selected.isNotEmpty(),onClick={runCatching{listenerState=store.native("listener_start",JSONObject().put("identity",secrets.identity()).put("trusts",JSONArray(peers)).put("label",label).put("scopes",JSONArray(selected.sorted())).put("allow_mesh",allowMesh))}.onFailure{message=it.message?:"Could not start listener"}}){Text("Enable receiving & create invitation")}
            listenerState.optJSONObject("pending")?.let{pending->val peer=pending.getJSONObject("trust");Text("Pair with ${peer.getString("label")}?",style=MaterialTheme.typography.titleMedium);Text(pending.getString("verification"));Text("Compare codes. Categories: ${peer.getJSONArray("scopes")}");Row{listOf(true,false).forEach{approved->TextButton(onClick={runCatching{if(approved){val next=peers.filter{it.getString("device")!=peer.getString("device")}+peer;secrets.saveTrusts(next);peers=next};store.native("listener_approve",JSONObject().put("id",pending.getString("id")).put("approved",approved));listenerState=store.native("listener_status")}.onFailure{message=it.message?:"Approval failed"}}){Text(if(approved)"Codes match — approve"else"Decline")}}}}
        }
        items(peers,key={it.getString("device")}){peer->Card{Column(Modifier.padding(12.dp)){Text(peer.getString("label"));Text(peer.getJSONArray("scopes").toString(),style=MaterialTheme.typography.bodySmall);val progressRows=snapshot.optJSONArray("peer_status")?:JSONArray();val progress=(0 until progressRows.length()).map{progressRows.getJSONObject(it)}.firstOrNull{it.getString("device")==peer.getString("device")};Text("Last exchange: ${progress?.optString("last_exchange")?:"Never"} · ${progress?.optInt("pending")?:0} queued journal entries",style=MaterialTheme.typography.bodySmall);TextButton(enabled=!busy,onClick={network("sync",JSONObject().put("trust",peer).put("address",address.ifBlank{peer.getString("endpoint")})){result->peer.put("endpoint",address.ifBlank{peer.getString("endpoint")});secrets.saveTrusts(peers);message="Synced ${result.getInt("changed")} incoming changes."}}){Text("Sync now")};TextButton(enabled=!busy,onClick={stopReceiving();runCatching{peers=peers.filter{it.getString("device")!=peer.getString("device")};secrets.saveTrusts(peers);store.native("forget",JSONObject().put("peer",peer.getString("device")));message="Local trust revoked. Revoke this phone from the desktop too; already shared copies remain."}.onFailure{message=it.message?:"Revocation failed"}}){Text("Revoke device")}}}}
        item{Text("Project summaries & plans",style=MaterialTheme.typography.titleMedium)}
        val records=snapshot.getJSONArray("records")
        items((0 until records.length()).map{records.getJSONObject(it)}.filter{it.getString("module") in listOf("projects","missions","sessions")},key={it.getString("id")}){record->Card{Column(Modifier.padding(12.dp)){Text(record.getString("title"));Text("${record.getString("module")} · ${record.getString("status")}",style=MaterialTheme.typography.labelSmall);Text(record.getString("summary"),maxLines=8,style=MaterialTheme.typography.bodySmall);if(record.getString("module")=="missions"&&record.getString("status") in listOf("draft","planned")){TextButton(enabled=!busy,onClick={runCatching{val next=JSONObject(record.toString()).put("status","planned");next.getJSONObject("fields").put("mobile_approval","Approved by ${store.device} at ${Instant.now()}");store.native("save_record",JSONObject().put("record",next));snapshot=store.snapshot();message="Plan approval saved. Sync to deliver it; no tool is automatically executed."}.onFailure{message=it.message?:"Approval failed"}}){Text("Approve plan for desktop review")}}}}}
        val conflicts=snapshot.getJSONArray("conflicts")
        item{Text("Conflicts: ${conflicts.length()}",style=MaterialTheme.typography.titleMedium)}
        items((0 until conflicts.length()).map{conflicts.getJSONObject(it)},key={it.getString("id")}){conflict->val local=conflict.getJSONObject("local");val incoming=conflict.getJSONObject("incoming");Column{Text(local.getString("title"));Text("Local: ${local.optString("body",local.optString("summary"))}",maxLines=8);Text("Incoming: ${incoming.optString("body",incoming.optString("summary"))}",maxLines=8);Row{listOf("local","incoming").forEach{choice->TextButton(enabled=!busy,onClick={runCatching{store.native("resolve",JSONObject().put("id",conflict.getString("id")).put("choice",choice).put("revision",local.getLong("revision")));snapshot=store.snapshot();onChanged()}.onFailure{message=it.message?:"Resolution failed"}}){Text("Use $choice")}}}}}
    }},confirmButton={TextButton(enabled=!busy,onClick=onDismiss){Text("Close")}})
}

@Composable fun MobileVaultDialog(store:LocalStore,context:Context,onDismiss:()->Unit){
    val secrets=remember{PeerSecrets(context,store.device)}
    var key by remember{mutableStateOf("")};var rows by remember{mutableStateOf(emptyList<JSONObject>())};var editing by remember{mutableStateOf<JSONObject?>(null)};var expected by remember{mutableStateOf<String?>(null)};var title by remember{mutableStateOf("")};var body by remember{mutableStateOf("")};var category by remember{mutableStateOf("document")};var password by remember{mutableStateOf("")};var message by remember{mutableStateOf("")};var busy by remember{mutableStateOf(false)};var generation by remember{mutableIntStateOf(0)};var exported by remember{mutableStateOf<ByteArray?>(null)};var pending by remember{mutableStateOf<ByteArray?>(null)};var preview by remember{mutableStateOf("")}
    fun lock(){generation++;key="";rows=emptyList();editing=null;expected=null;title="";body="";password="";preview="";busy=false}
    fun apply(value:JSONObject){val a=value.getJSONArray("entries");rows=(0 until a.length()).map{a.getJSONObject(it)}}
    fun operation(name:String,args:JSONObject=JSONObject(),done:(JSONObject)->Unit){val request=JSONObject(args.toString()).put("key",key);val started=generation;busy=true;Thread{val result=runCatching{store.native(name,request)};android.os.Handler(android.os.Looper.getMainLooper()).post{if(generation==started){busy=false;result.onSuccess{runCatching{done(it)}.onFailure{message=it.message?:"Vault operation failed"}}.onFailure{message=it.message?:"Vault operation failed"}}}}.start()}
    fun refresh(){operation("vault_list"){apply(it)}}
    DisposableEffect(Unit){val lifecycle=(context as? ComponentActivity)?.lifecycle;val observer=androidx.lifecycle.LifecycleEventObserver{_,event->if(event==androidx.lifecycle.Lifecycle.Event.ON_STOP)lock()};lifecycle?.addObserver(observer);onDispose{lifecycle?.removeObserver(observer);lock()}}
    DisposableEffect(key){val handler=android.os.Handler(android.os.Looper.getMainLooper());val timeout=Runnable{lock();message="Vault session expired after ten minutes."};if(key.isNotEmpty())handler.postDelayed(timeout,600000);onDispose{handler.removeCallbacks(timeout)}}
    val export=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")){uri->if(uri!=null){runCatching{val bytes=exported?:error("Vault session closed; generate the backup again");context.contentResolver.openOutputStream(uri)?.use{it.write(bytes)}?:error("Cannot open backup destination")}.onSuccess{message="Encrypted vault archive exported. Keep its password separately."}.onFailure{message=it.message?:"Export failed"}};exported=null}
    val importer=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->if(uri!=null){runCatching{context.contentResolver.openInputStream(uri)?.use{stream->val output=java.io.ByteArrayOutputStream();val buffer=ByteArray(8192);while(true){val count=stream.read(buffer);if(count<0)break;require(output.size()+count<=20*1024*1024){"Private archive exceeds 20 MiB"};output.write(buffer,0,count)};output.toByteArray()}?:error("Cannot open archive")}.onSuccess{bytes->pending=bytes;preview="";message="Encrypted archive selected. Unlock again if the picker locked the vault, enter its password, then preview before importing."}.onFailure{message=it.message?:"Import preview failed"}}}
    androidx.compose.ui.window.Dialog(onDismissRequest={lock();onDismiss()},properties=androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth=false,securePolicy=androidx.compose.ui.window.SecureFlagPolicy.SecureOn)){
      Surface(Modifier.fillMaxSize()){LazyColumn(Modifier.padding(20.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
        item{Row{Text("Private vault",style=MaterialTheme.typography.headlineSmall,modifier=Modifier.weight(1f));TextButton(onClick={lock();onDismiss()}){Text("Close")}};Text("Device-local AES-GCM private records. Android Keystore protects the device key. This view blocks screenshots and locks when backgrounded, closed or after ten minutes. Ordinary search, AI and peer sync exclude private records. Export an encrypted backup before uninstalling.",style=MaterialTheme.typography.bodySmall)}
        if(key.isEmpty())item{Button(enabled=!busy,onClick={runCatching{key=secrets.vaultKey(store);refresh()}.onFailure{lock();message=it.message?:"Cannot unlock vault"}}){Text("Unlock private vault")}}
        else{
          item{Row{TextButton(onClick={lock()}){Text("Lock")};TextButton(enabled=!busy,onClick={val now=Instant.now().toString();editing=JSONObject().put("id",UUID.randomUUID().toString()).put("title","").put("body","").put("category","document").put("amount",JSONObject.NULL).put("due",JSONObject.NULL).put("filename",JSONObject.NULL).put("file_base64",JSONObject.NULL).put("updated_at",now);expected=null;title="";body="";category="document"}){Text("New private record")}}}
          items(rows,key={it.getString("id")}){entry->TextButton(enabled=!busy,onClick={editing=JSONObject(entry.toString());expected=entry.getString("updated_at");title=entry.getString("title");body=entry.getString("body");category=entry.getString("category")}){Text("${entry.getString("title")} · ${entry.getString("category")}")}}
          if(editing!=null)item{OutlinedTextField(value=title,onValueChange={title=it},label={Text("Private title")},modifier=Modifier.fillMaxWidth());Row{listOf("document","finance","company","subscription").forEach{cat->FilterChip(selected=category==cat,onClick={category=cat},label={Text(cat)})}};OutlinedTextField(value=body,onValueChange={body=it},label={Text("Private text")},minLines=5,modifier=Modifier.fillMaxWidth());if(!editing!!.isNull("file_base64"))Text("Attached original is retained when editing text.",style=MaterialTheme.typography.bodySmall);Row{Button(enabled=!busy&&title.isNotBlank(),onClick={val entry=JSONObject(editing!!.toString()).put("title",title).put("body",body).put("category",category);val args=JSONObject().put("entry",entry);expected?.let{args.put("expected_updated_at",it)};operation("vault_save",args){editing=null;title="";body="";refresh();message="Private record encrypted and saved."}}){Text("Save encrypted")};TextButton(onClick={editing=null;title="";body=""}){Text("Cancel edit")}}}
          item{HorizontalDivider();Text("Encrypted portable backup",style=MaterialTheme.typography.titleMedium);OutlinedTextField(value=password,onValueChange={password=it;preview=""},label={Text("Backup password (12+ characters)")},visualTransformation=androidx.compose.ui.text.input.PasswordVisualTransformation(),modifier=Modifier.fillMaxWidth());Text("Vault-only archives are compatible with desktop encrypted backups. Keep the password separately. Restore never silently overwrites private records.",style=MaterialTheme.typography.bodySmall);Row{TextButton(enabled=!busy&&password.codePointCount(0,password.length)>=12,onClick={operation("vault_export",JSONObject().put("password",password)){exported=android.util.Base64.decode(it.getString("archive_base64"),android.util.Base64.NO_WRAP);password="";export.launch("command-center-private.pacc")}}){Text("Export encrypted")};TextButton(enabled=!busy,onClick={importer.launch(arrayOf("application/octet-stream","application/zip","*/*"))}){Text("Choose archive")}}}
          if(pending!=null&&preview.isEmpty())item{TextButton(enabled=!busy&&password.isNotEmpty(),onClick={operation("vault_preview_import",JSONObject().put("password",password).put("archive_base64",android.util.Base64.encodeToString(pending!!,android.util.Base64.NO_WRAP))){preview=it.toString(2)}}){Text("Preview selected archive with this password")};TextButton(onClick={pending=null;preview=""}){Text("Discard selected encrypted archive")}}
          if(preview.isNotEmpty())item{Text(preview,style=MaterialTheme.typography.bodySmall);TextButton(enabled=!busy&&pending!=null,onClick={operation("vault_import",JSONObject().put("password",password).put("archive_base64",android.util.Base64.encodeToString(pending!!,android.util.Base64.NO_WRAP)).put("confirmation","IMPORT PRIVATE COPIES")){message="Imported ${it.getInt("imported")} private records";pending=null;preview="";password="";refresh()}}){Text("Import reviewed private records as safe copies")};TextButton(onClick={pending=null;preview=""}){Text("Cancel import")}}
        }
        if(busy)item{Text("Working with encrypted local data…")}
        if(message.isNotEmpty())item{Text(message)}
      }}
    }
}

class TaskReminderReceiver:android.content.BroadcastReceiver(){
    companion object{
        private const val CHANNEL="pcc-task-reminders"
        fun enabled(context:Context)=context.getSharedPreferences("reminders",Context.MODE_PRIVATE).getBoolean("enabled",false)
        private fun pending(context:Context)=android.app.PendingIntent.getBroadcast(context,1401,Intent(context,TaskReminderReceiver::class.java).setAction("com.personal.commandcenter.REMIND"),android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_IMMUTABLE)
        fun schedule(context:Context){val alarm=context.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager;alarm.cancel(pending(context));if(!enabled(context))return;val fire=java.util.Calendar.getInstance().apply{set(java.util.Calendar.HOUR_OF_DAY,9);set(java.util.Calendar.MINUTE,0);set(java.util.Calendar.SECOND,0);set(java.util.Calendar.MILLISECOND,0);if(timeInMillis<=System.currentTimeMillis())add(java.util.Calendar.DATE,1)};alarm.setAndAllowWhileIdle(android.app.AlarmManager.RTC_WAKEUP,fire.timeInMillis,pending(context))}
        fun setEnabled(context:Context,enabled:Boolean){context.getSharedPreferences("reminders",Context.MODE_PRIVATE).edit().putBoolean("enabled",enabled).apply();schedule(context);if(!enabled)(context.getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager).cancel(1401)}
    }
    override fun onReceive(context:Context,intent:Intent){
        schedule(context);if(intent.action!="com.personal.commandcenter.REMIND"||!enabled(context))return
        val result=goAsync();Thread{try{runCatching{
            val manager=context.getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager
            manager.createNotificationChannel(android.app.NotificationChannel(CHANNEL,"Task review reminders",android.app.NotificationManager.IMPORTANCE_DEFAULT))
            if(!manager.areNotificationsEnabled()||manager.getNotificationChannel(CHANNEL)?.importance==android.app.NotificationManager.IMPORTANCE_NONE)return@runCatching
            val now=java.time.LocalTime.now();if(now.hour<8||now.hour>=22)return@runCatching
            val due=LocalStore(context).list().any{it.optString("kind")=="task"&&it.optString("status")=="active"&&!it.isNull("due")&&it.getString("due")<=LocalDate.now().toString()}
            if(due){val open=android.app.PendingIntent.getActivity(context,1402,Intent(context,MainActivity::class.java),android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_IMMUTABLE);manager.notify(1401,android.app.Notification.Builder(context,CHANNEL).setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle("Review your due tasks").setContentText("Open Command Center to review outstanding work.").setContentIntent(open).setAutoCancel(true).setVisibility(android.app.Notification.VISIBILITY_PRIVATE).build())}
        }}finally{result.finish()}}.start()
    }
}
@Composable fun MobileReminderControls(context:Context){
    var enabled by remember{mutableStateOf(TaskReminderReceiver.enabled(context))};var message by remember{mutableStateOf("")}
    fun enable(){TaskReminderReceiver.setEnabled(context,true);enabled=true;message="Local task review enabled. Delivery depends on Android notification and battery settings."}
    val permission=rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()){allowed->if(allowed)enable()else message="Notifications were not allowed. Enable permission in Android Settings to receive reminders."}
    Column{Text("Local task reminders",style=MaterialTheme.typography.titleSmall);Text("Opt-in generic task review around 9 AM. Android may delay inexact alarms; quiet hours are 10 PM–8 AM. Restart and timezone changes reschedule while enabled. Force-stop blocks delivery until reopening. No task titles or remote push service; these digests do not suppress another device’s alerts.",style=MaterialTheme.typography.bodySmall);TextButton(onClick={if(enabled){TaskReminderReceiver.setEnabled(context,false);enabled=false;message="Local task reminders disabled."}else if(android.os.Build.VERSION.SDK_INT>=33)permission.launch(Manifest.permission.POST_NOTIFICATIONS)else enable()}){Text(if(enabled)"Disable local reminders"else"Request permission & enable reminders")};if(message.isNotEmpty())Text(message,style=MaterialTheme.typography.bodySmall)}
}


@Composable fun MobileAttachmentsDialog(store:LocalStore,context:Context,onDismiss:()->Unit){
    var rows by remember{mutableStateOf<List<JSONObject>>(emptyList())};var query by remember{mutableStateOf("")};var message by remember{mutableStateOf("")};var exported by remember{mutableStateOf<ByteArray?>(null)}
    fun load(){runCatching{val a=store.native("attachments",JSONObject().put("query",query)).getJSONArray("rows");rows=(0 until a.length()).map{a.getJSONObject(it)}}.onFailure{message=it.message?:"Cannot load attachments"}}
    val save=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")){uri->if(uri!=null)runCatching{val bytes=exported?:error("Choose the file again");context.contentResolver.openOutputStream(uri)?.use{it.write(bytes)}?:error("Cannot open destination")}.onSuccess{message="Original exported as a plaintext file. Keep the copy private."}.onFailure{message=it.message?:"Export failed"};exported=null}
    LaunchedEffect(Unit){load()}
    AlertDialog(onDismissRequest=onDismiss,title={Text("Synced attachments")},text={LazyColumn(verticalArrangement=Arrangement.spacedBy(10.dp)){
        item{Text("Ordinary files only. Pair with attachment and owner categories enabled, then sync. Exports are plaintext copies; unknown files are not executed here.");OutlinedTextField(query,{query=it.take(300)},label={Text("Filename / indexed text")});TextButton(onClick={load()}){Text("Search / refresh")}}
        items(rows,key={it.getString("id")}){row->Column{Text(row.getString("filename"));Text("Owner: ${row.getString("owner_id")}",style=MaterialTheme.typography.bodySmall);Text("SHA-256: ${row.getString("sha256")}",style=MaterialTheme.typography.bodySmall);TextButton(onClick={runCatching{val a=store.native("attachment",JSONObject().put("id",row.getString("id")));exported=android.util.Base64.decode(a.getString("content_base64"),android.util.Base64.DEFAULT);save.launch(a.getString("filename"))}.onFailure{message=it.message?:"Could not read attachment"}}){Text("Export original")}}}
        item{if(rows.isEmpty())Text("No matching attachments.");Text(message)}
    }},confirmButton={TextButton(onClick=onDismiss){Text("Close")}})
}
