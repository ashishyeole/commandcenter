import Foundation
import Speech
import AVFoundation
import AppKit
import Darwin

final class VoiceState {
    static let shared=VoiceState()
    private let lock=NSLock()
    private var phase="idle",text="",error=""
    func set(_ phase:String,_ text:String?=nil,_ error:String="") {lock.lock();defer{lock.unlock()};self.phase=phase;if let text{self.text=String(text.prefix(12000))};self.error=error}
    func json()->String {lock.lock();defer{lock.unlock()};let value:[String:Any]=["supported":true,"phase":phase,"text":text,"error":error,"engine":"macOS on-device speech"];return String(data:(try? JSONSerialization.data(withJSONObject:value)) ?? Data("{}".utf8),encoding:.utf8) ?? "{}"}
}
final class VoiceCapture {
    static let shared=VoiceCapture()
    private var generation=0
    private var engine:AVAudioEngine?
    private var request:SFSpeechAudioBufferRecognitionRequest?
    private var task:SFSpeechRecognitionTask?
    private var recognizer:SFSpeechRecognizer?
    private var observer:NSObjectProtocol?
    func stop(clear:Bool=false) {
        generation+=1
        if let engine {engine.stop();engine.inputNode.removeTap(onBus:0)}
        request?.endAudio();task?.cancel();engine=nil;request=nil;task=nil;recognizer=nil
        if let observer{NotificationCenter.default.removeObserver(observer);self.observer=nil}
        VoiceState.shared.set("finished",clear ? "":nil)
    }
    func start() {
        stop(clear:true);let ticket=generation;VoiceState.shared.set("permission","")
        AVCaptureDevice.requestAccess(for:.audio){granted in DispatchQueue.main.async {
            guard ticket==self.generation else{return}
            guard granted else{VoiceState.shared.set("error","","Microphone permission denied. Review macOS Privacy settings.");return}
            SFSpeechRecognizer.requestAuthorization{status in DispatchQueue.main.async {
                guard ticket==self.generation else{return}
                guard status == .authorized else{VoiceState.shared.set("error","","Speech recognition permission denied.");return}
                self.begin(ticket)
            }}
        }}
    }
    private func begin(_ ticket:Int) {
        guard let recognizer=SFSpeechRecognizer(locale:Locale.current),recognizer.isAvailable,recognizer.supportsOnDeviceRecognition else{VoiceState.shared.set("error","","On-device recognition is unavailable for this locale. No cloud fallback is used.");return}
        let request=SFSpeechAudioBufferRecognitionRequest();request.requiresOnDeviceRecognition=true;request.shouldReportPartialResults=true
        let engine=AVAudioEngine();let input=engine.inputNode;let format=input.outputFormat(forBus:0)
        guard format.sampleRate>0,format.channelCount>0 else{VoiceState.shared.set("error","","No microphone input is available.");return}
        input.installTap(onBus:0,bufferSize:1024,format:format){buffer,_ in request.append(buffer)}
        self.engine=engine;self.request=request;self.recognizer=recognizer
        do{engine.prepare();try engine.start()}catch{stop();VoiceState.shared.set("error","","Could not start microphone capture.");return}
        VoiceState.shared.set("recording","")
        observer=NotificationCenter.default.addObserver(forName:NSApplication.didResignActiveNotification,object:nil,queue:.main){_ in self.stop()}
        task=recognizer.recognitionTask(with:request){result,error in DispatchQueue.main.async {
            guard ticket==self.generation else{return}
            if let result{VoiceState.shared.set("recording",result.bestTranscription.formattedString)}
            if error != nil || result?.isFinal == true{self.stop();if error != nil && result == nil{VoiceState.shared.set("error",nil,"Recognition stopped. Check microphone and local language support.")}}
        }}
        DispatchQueue.main.asyncAfter(deadline:.now()+60){if ticket==self.generation{self.stop()}}
    }
}
@_cdecl("pcc_voice_start") public func pcc_voice_start(){VoiceState.shared.set("permission","");DispatchQueue.main.async{VoiceCapture.shared.start()}}
@_cdecl("pcc_voice_stop") public func pcc_voice_stop(){DispatchQueue.main.async{VoiceCapture.shared.stop()}}
@_cdecl("pcc_voice_clear") public func pcc_voice_clear(){VoiceState.shared.set("idle","");DispatchQueue.main.async{VoiceCapture.shared.stop(clear:true)}}
@_cdecl("pcc_voice_snapshot") public func pcc_voice_snapshot()->UnsafeMutablePointer<CChar>?{strdup(VoiceState.shared.json())}
@_cdecl("pcc_voice_free") public func pcc_voice_free(_ value:UnsafeMutablePointer<CChar>?){free(value)}
