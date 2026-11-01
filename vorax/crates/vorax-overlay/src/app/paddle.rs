//! One persistent CPU OCR worker. Only text crops cross the local pipe; no shell or network calls.
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use serde_json::Value;
use vorax_scan::pixels::Img;

pub struct Paddle {
    child: Child,
    input: ChildStdin,
    replies: Receiver<Result<Value, String>>,
    pub startup: Value,
    pub last: Value,
}

impl Paddle {
    pub fn new() -> Result<Self, String> {
        let exe = std::env::var_os("VORAX_OCR_HOST").map(std::path::PathBuf::from)
            .or_else(||std::env::current_exe().ok().and_then(|p|p.parent().map(|p|p.join("Vorax.OcrHost.exe"))))
            .ok_or("cannot locate the bundled OCR worker")?;
        if !exe.is_file() {return Err("Vorax.OcrHost.exe is missing; extract the complete OCR preview ZIP".into());}
        let threads = std::env::var("VORAX_OCR_THREADS").ok().and_then(|s|s.parse::<u32>().ok()).unwrap_or(2).clamp(1,4);
        let mut child=Command::new(&exe).args(["--serve","--threads",&threads.to_string()])
            .current_dir(exe.parent().ok_or("OCR worker has no directory")?)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .creation_flags(0x08000000).spawn().map_err(|e|format!("start OCR worker: {e}"))?;
        let input=child.stdin.take().ok_or("OCR worker stdin unavailable")?;
        let mut output=child.stdout.take().ok_or("OCR worker stdout unavailable")?;
        let (tx,replies)=channel();
        std::thread::spawn(move || loop {
            let mut header=[0u8;4];
            let result=output.read_exact(&mut header).map_err(|e|format!("OCR worker pipe: {e}")).and_then(|_| {
                let length=u32::from_le_bytes(header) as usize;
                if length==0 || length>1024*1024 {return Err("invalid OCR reply length".into());}
                let mut bytes=vec![0;length];output.read_exact(&mut bytes).map_err(|e|e.to_string())?;
                serde_json::from_slice(&bytes).map_err(|e|format!("invalid OCR response: {e}"))
            });
            let failed=result.is_err();if tx.send(result).is_err() || failed {break;}
        });
        let mut worker=Self{child,input,replies,startup:Value::Null,last:Value::Null};
        let ready=worker.receive(Duration::from_secs(30))?;
        if ready["ready"]!=true || ready["backend"]!="paddleocr-v5-korean-cpu" {return Err("unexpected OCR worker handshake".into());}
        worker.startup=ready;Ok(worker)
    }

    fn receive(&mut self,timeout:Duration)->Result<Value,String> {
        let value=self.replies.recv_timeout(timeout).map_err(|e|format!("OCR worker timed out or stopped: {e}"))??;
        if let Some(error)=value.get("error").and_then(Value::as_str) {return Err(format!("PaddleOCR: {error}"));}
        Ok(value)
    }

    pub fn read(&mut self,img:&Img)->Result<String,String> {
        let length=img.width.checked_mul(img.height).and_then(|n|n.checked_mul(4)).ok_or("OCR image too large")?;
        if img.width==0 || img.height==0 || img.width>4096 || img.height>4096 || length>16*1024*1024 || img.data.len()!=length {return Err("invalid OCR image size".into());}
        for v in [img.width as u32,img.height as u32,length as u32] {self.input.write_all(&v.to_le_bytes()).map_err(|e|e.to_string())?;}
        self.input.write_all(&img.data).and_then(|_|self.input.flush()).map_err(|e|e.to_string())?;
        let value=self.receive(Duration::from_secs(15));
        if value.is_err() {let _=self.child.kill();}
        let value=value?;let text=value["text"].as_str().ok_or("missing OCR text in response")?.to_string();
        self.last=value;Ok(text)
    }
}

impl Drop for Paddle {
    fn drop(&mut self) {let _=self.child.kill();let _=self.child.wait();}
}
