package com.personal.commandcenter

import android.app.*
import android.content.*
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.text.input.PasswordVisualTransformation
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.json.JSONObject
import java.net.URI
import java.util.UUID
import javax.net.ssl.HttpsURLConnection

class PushApplication:Application(){override fun onCreate(){super.onCreate();if(NativePush.enabled(this))runCatching{NativePush.initialize(this)}}}
object NativePush {
    private val lock=Any()
    private fun prefs(c:Context)=c.getSharedPreferences("native-push-state",Context.MODE_PRIVATE)
    fun enabled(c:Context)=prefs(c).getBoolean("enabled",false)
    fun config(c:Context)=JSONObject(PeerSecrets(c,"native-push").pushConfig()?:"{}")
    fun status(c:Context)=prefs(c).getString("status","Native push is not configured.")!!
    fun status(c:Context,s:String){prefs(c).edit().putString("status",s).apply()}
    fun initialize(c:Context){
        if(!enabled(c))return
        val cfg=config(c)
        if(FirebaseApp.getApps(c).isEmpty())FirebaseApp.initializeApp(c,FirebaseOptions.Builder().setApplicationId(cfg.getString("appId")).setApiKey(cfg.getString("apiKey")).setProjectId(cfg.getString("project")).setGcmSenderId(cfg.getString("sender")).build())
        FirebaseMessaging.getInstance().isAutoInitEnabled=true
        val manager=c.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel("pcc-native-push","Native task review",NotificationManager.IMPORTANCE_DEFAULT))
        FirebaseMessaging.getInstance().token.addOnSuccessListener{token->upload(c,token)}.addOnFailureListener{status(c,"Firebase registration failed; review configuration and Play services.")}
    }
    private fun request(cfg:JSONObject,method:String,path:String,body:JSONObject?=null){
        val base=URI(cfg.getString("origin"));require(base.scheme=="https"&&base.host!=null&&base.rawUserInfo==null&&base.rawQuery==null&&base.rawFragment==null&&(base.path.isNullOrEmpty()||base.path=="/")){"Use an HTTPS origin without a path or credentials"}
        val conn=URI(cfg.getString("origin").trimEnd('/')+path).toURL().openConnection() as HttpsURLConnection
        try{conn.instanceFollowRedirects=false;conn.connectTimeout=15000;conn.readTimeout=20000;conn.requestMethod=method;conn.setRequestProperty("Authorization","Bearer "+cfg.getString("secret"));conn.setRequestProperty("Content-Type","application/json")
            if(body!=null){val data=body.toString().toByteArray();conn.doOutput=true;conn.setFixedLengthStreamingMode(data.size);conn.outputStream.use{it.write(data)}}
            require(conn.responseCode in 200..299){"Push host did not confirm request (HTTP ${conn.responseCode})"}
            conn.inputStream.use{input->val bytes=ByteArray(4096);var size=0;while(true){val n=input.read(bytes);if(n<0)break;size+=n;require(size<=65536){"Push response too large"}}}
        }finally{conn.disconnect()}
    }
    fun register(c:Context,cfg:JSONObject){synchronized(lock){
        val old=config(c);require(!old.has("origin")||old.getString("origin")==cfg.getString("origin")){"Remove the previous host registration first"}
        val uri=URI(cfg.getString("origin"));require(uri.scheme=="https"&&uri.host!=null&&uri.rawUserInfo==null&&uri.rawQuery==null&&uri.rawFragment==null&&(uri.path.isNullOrEmpty()||uri.path=="/")){"Use an HTTPS origin"}
        require(cfg.getString("secret").length in 32..4096&&!cfg.getString("secret").contains('\n')&&!cfg.getString("secret").contains('\r')){"Use a strong host access token"}
        for(k in listOf("appId","apiKey","project","sender"))require(cfg.getString(k).length in 1..300){"Complete the Firebase client configuration"}
        // Changing Firebase project needs a process restart; never silently reuse another project's default app.
        if(old.has("appId"))require(old.getString("appId")==cfg.getString("appId")){"Remove push configuration and restart before changing Firebase projects"}
        if(FirebaseApp.getApps(c).isNotEmpty())require(FirebaseApp.getInstance().options.applicationId==cfg.getString("appId")){"Restart the app before changing Firebase projects"}
        cfg.put("id",old.optString("id").ifEmpty{UUID.randomUUID().toString()})
        PeerSecrets(c,"native-push").savePushConfig(cfg.toString());prefs(c).edit().putBoolean("enabled",true).commit();status(c,"Requesting Firebase registration; no private data is uploaded.")
        initialize(c)
    }}
    fun upload(c:Context,token:String){Thread{synchronized(lock){if(enabled(c))runCatching{val cfg=config(c);request(cfg,"POST","/v1/push/devices",JSONObject().put("id",cfg.getString("id")).put("platform","fcm").put("environment","production").put("token",token).put("consent",true));status(c,"Registered. Device ID: ${cfg.getString("id")}")}.onFailure{status(c,"Push registration needs retry. Reopen or register again; check your host.")}}}.start()}
    fun disable(c:Context){prefs(c).edit().putBoolean("enabled",false).commit();if(FirebaseApp.getApps(c).isNotEmpty())FirebaseMessaging.getInstance().isAutoInitEnabled=false;synchronized(lock){
        val cfg=config(c);var failure:Throwable?=null
        if(cfg.has("id"))runCatching{request(cfg,"DELETE","/v1/push/devices/"+cfg.getString("id"))}.onFailure{failure=it}
        if(FirebaseApp.getApps(c).isNotEmpty()){FirebaseMessaging.getInstance().isAutoInitEnabled=false;FirebaseMessaging.getInstance().deleteToken()}
        c.getSystemService(NotificationManager::class.java).cancel(1402)
        if(failure==null){PeerSecrets(c,"native-push").savePushConfig("{}");status(c,"Remote registration removed. Push disabled; restart before changing Firebase projects.")}
        else{status(c,"Local push disabled; remote removal unconfirmed. Use Remove again to retry.");throw failure!!}
    }}
}
class NativePushService:FirebaseMessagingService(){
    override fun onNewToken(token:String){if(NativePush.enabled(this))NativePush.upload(this,token)}
    override fun onMessageReceived(message:RemoteMessage){
        if(!NativePush.enabled(this)||message.data["kind"]!="task_review")return
        val event=message.data["pcc_event"]?:return
        if(!event.matches(Regex("[A-Za-z0-9_-]{1,100}")))return
        val hour=java.time.LocalTime.now().hour;if(hour<8||hour>=22)return
        if(Build.VERSION.SDK_INT>=33&&checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)return
        val preferences=getSharedPreferences("native-push-events",MODE_PRIVATE)
        synchronized(NativePushService::class.java){if(preferences.contains(event))return;val now=System.currentTimeMillis();val edit=preferences.edit();preferences.all.forEach{(k,v)->if((v as? Long?:0)<now-86400000)edit.remove(k)};edit.putLong(event,now).commit()}
        val intent=PendingIntent.getActivity(this,1402,Intent(this,MainActivity::class.java),PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val manager=getSystemService(NotificationManager::class.java);manager.createNotificationChannel(NotificationChannel("pcc-native-push","Native task review",NotificationManager.IMPORTANCE_DEFAULT))
        manager.notify(1402,Notification.Builder(this,"pcc-native-push").setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle("Personal Command Center").setContentText("A task needs your attention. Open the app to review your due tasks.").setContentIntent(intent).setAutoCancel(true).setVisibility(Notification.VISIBILITY_PRIVATE).build())
    }
}
@Composable fun NativePushControls(context:Context){
    var expanded by remember{mutableStateOf(false)};var origin by remember{mutableStateOf("")};var secret by remember{mutableStateOf("")};var appId by remember{mutableStateOf("")};var apiKey by remember{mutableStateOf("")};var project by remember{mutableStateOf("")};var sender by remember{mutableStateOf("")};var consent by remember{mutableStateOf(false)};var busy by remember{mutableStateOf(false)};var message by remember{mutableStateOf(NativePush.status(context))}
    fun work(fn:()->Unit){busy=true;Thread{val result=runCatching(fn);android.os.Handler(android.os.Looper.getMainLooper()).post{busy=false;message=result.exceptionOrNull()?.message?:NativePush.status(context);secret="";consent=false}}.start()}
    fun enable(){if(consent)work{NativePush.register(context,JSONObject().put("origin",origin).put("secret",secret).put("appId",appId).put("apiKey",apiKey).put("project",project).put("sender",sender))}}
    val permission=rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()){granted->if(granted)enable()else message="Notification permission denied"}
    Column{TextButton(onClick={expanded=!expanded;if(!expanded){secret="";consent=false}}){Text("Native push notifications")}
        if(expanded){Text("Optional Firebase and your HTTPS push host receive the device token. Only generic alerts are sent. Requires Play services. Android quiet hours: 10 PM–8 AM; missed quiet-hour alerts are dropped. No Analytics SDK is included.")
            OutlinedTextField(origin,{origin=it;consent=false},enabled=!busy,label={Text("HTTPS host origin")});OutlinedTextField(secret,{secret=it;consent=false},enabled=!busy,label={Text("Host member access token")},visualTransformation=PasswordVisualTransformation())
            OutlinedTextField(appId,{appId=it;consent=false},enabled=!busy,label={Text("Firebase mobile SDK app ID")});OutlinedTextField(apiKey,{apiKey=it;consent=false},enabled=!busy,label={Text("Firebase client API key")});OutlinedTextField(project,{project=it;consent=false},enabled=!busy,label={Text("Firebase project ID")});OutlinedTextField(sender,{sender=it;consent=false},enabled=!busy,label={Text("Firebase sender / project number")})
            androidx.compose.foundation.layout.Row{Checkbox(consent,{consent=it},enabled=!busy);Text("Approve token registration and generic notifications")}
            TextButton(enabled=!busy&&consent,onClick={if(Build.VERSION.SDK_INT>=33)permission.launch(android.Manifest.permission.POST_NOTIFICATIONS)else enable()}){Text("Register / refresh native push")}
            TextButton(enabled=!busy,onClick={work{NativePush.disable(context)}}){Text("Remove registration & disable")};TextButton(onClick={message=NativePush.status(context)}){Text("Refresh registration status / device ID")};Text(message)
        }
    }
    DisposableEffect(Unit){onDispose{secret="";consent=false}}
}
