//! Real keyboard-driven in-kernel Samen terminal. Commands execute against
//! the mounted SamenFS1 block device, not simulated text or a browser mockup.
use core::fmt::{self, Write};
use crate::{ata::AtaDisk,keyboard::Key};
use samen_kernel::fs::{BlockDevice,Fs};
const LINES:usize=18;
const COLS:usize=105;
#[derive(Clone,Copy)]
pub struct Line {data:[u8;COLS],len:usize}
impl Line {
    const EMPTY:Self=Self{data:[0;COLS],len:0};
    pub fn text(&self)->&str{core::str::from_utf8(&self.data[..self.len]).unwrap_or("?")}
}
struct LineWriter(Line);
impl Write for LineWriter {
    fn write_str(&mut self,s:&str)->fmt::Result {
        for b in s.bytes().filter(|b|b.is_ascii()){
            if self.0.len>=COLS{break;}
            self.0.data[self.0.len]=b;
            self.0.len+=1;
        }
        Ok(())
    }
}
pub struct Terminal {
    pub input:[u8;128],
    pub input_len:usize,
    lines:[Line;LINES],
    next:usize,
    pub count:usize,
    pub executions:u32,
    last:[u8;24],
    last_len:usize,
}
impl Terminal {
    pub fn new()->Self {
        let mut terminal=Self{
            input:[0;128],input_len:0,lines:[Line::EMPTY;LINES],
            next:0,count:0,executions:0,last:[0;24],last_len:0,
        };
        terminal.say("Samen Terminal 0.4  |  x86-64 Rust");
        terminal.say("Type help for commands.  Files are persistent.");
        terminal
    }
    fn push(&mut self,line:Line) {
        self.lines[self.next]=line;
        self.next=(self.next+1)%LINES;
        self.count=self.count.saturating_add(1).min(LINES);
    }
    pub fn say(&mut self,msg:&str) {
        let mut l=LineWriter(Line::EMPTY);
        let _=l.write_str(msg);
        self.push(l.0);
    }
    pub fn output(&mut self,args:fmt::Arguments<'_>){
        let mut l=LineWriter(Line::EMPTY);
        let _=l.write_fmt(args);
        self.push(l.0);
    }
    pub fn lines(&self)->impl Iterator<Item=&str>{
        (0..self.count).map(|n|{
            let at=(self.next+LINES-self.count+n)%LINES;
            self.lines[at].text()
        })
    }
    pub fn command_line(&self)->&str {
        core::str::from_utf8(&self.input[..self.input_len]).unwrap_or("")
    }
    pub fn last_command(&self)->&str{
        core::str::from_utf8(&self.last[..self.last_len]).unwrap_or("")
    }
    pub fn key(&mut self,key:Key,disk:&mut Option<AtaDisk>,fs:&mut Option<Fs>)->bool{
        match key {
            Key::Char(ch)=>{
                if ch.is_ascii() && !ch.is_ascii_control() && self.input_len<self.input.len()-1 {
                    self.input[self.input_len]=ch;
                    self.input_len+=1;
                    true
                }else{false}
            }
            Key::Backspace=>{
                if self.input_len>0{self.input_len-=1;true}else{false}
            }
            Key::Enter=>{
                let mut command=[0u8;128];
                command[..self.input_len].copy_from_slice(&self.input[..self.input_len]);
                let len=self.input_len;
                self.input_len=0;
                let text=core::str::from_utf8(&command[..len]).unwrap_or("");
                self.run(text,disk,fs);
                true
            }
        }
    }
    pub fn run(&mut self,text:&str,disk:&mut Option<AtaDisk>,fs:&mut Option<Fs>){
        let text=text.trim();
        if text.is_empty(){return;}
        self.executions=self.executions.saturating_add(1);
        let mut prompt=LineWriter(Line::EMPTY);
        let _=write!(&mut prompt,"samen:~$ {}",text);
        self.push(prompt.0);
        self.last_len=text.len().min(self.last.len());
        self.last[..self.last_len].copy_from_slice(&text.as_bytes()[..self.last_len]);
        let (cmd,args)=text.split_once(' ').unwrap_or((text,""));
        match cmd {
            "help"=>{
                self.say("help  clear  ls  cat FILE  write FILE TEXT");
                self.say("rm FILE  stat FILE  disk  about  echo TEXT");
            }
            "clear"=>{self.lines=[Line::EMPTY;LINES];self.count=0;self.next=0;}
            "echo"=>self.say(args),
            "about"=>self.say("Samen native x86-64 kernel / Rust / SamenFS1"),
            "disk"=>match disk.as_ref(){
                Some(d)=>self.output(format_args!("ATA primary master: {} sectors",d.sector_count())),
                None=>self.say("No IDE disk attached"),
            },
            "ls"=>match fs.as_ref(){
                Some(f)=>{
                    let mut any=false;
                    for info in f.files(){
                        let mut writer=LineWriter(Line::EMPTY);
                        let _=write!(&mut writer,"{:>7}  {}",info.size,info.name());
                        self.push(writer.0);
                        any=true;
                    }
                    if !any{self.say("(empty)");}
                }
                None=>self.say("SamenFS1 not mounted"),
            },
            "cat"=>{
                let mut buffer=[0u8;COLS];
                let mut count=0usize;
                let mut truncated=false;
                match (disk.as_mut(),fs.as_ref()){
                    (Some(d),Some(f))=>{
                        let result=f.read_file(d,args.trim(),|part|{
                            let n=part.len().min(COLS-count);
                            buffer[count..count+n].copy_from_slice(&part[..n]);
                            count+=n;
                            if n<part.len(){truncated=true;}
                        });
                        match result {
                            Ok(())=>{
                                let display=core::str::from_utf8(&buffer[..count]).unwrap_or("[binary data]");
                                self.say(display);
                                if truncated { self.say("[output truncated; file is larger than display buffer]"); }
                            }
                            Err(err)=>self.output(format_args!("cat: {:?}",err))
                        }
                    }
                    _=>self.say("SamenFS1 not mounted"),
                }
            },
            "stat"=>match fs.as_ref().and_then(|f|f.info(args.trim()).ok()){
                Some(f)=>self.output(format_args!("{}: {} bytes at sector {}",f.name(),f.size,f.first_sector)),
                None=>self.say("File not found"),
            },
            "write"=>{
                let (name,data)=args.trim().split_once(' ').unwrap_or((args.trim(),""));
                match (disk.as_mut(),fs.as_mut()){
                    (Some(d),Some(f))=>match f.write_file(d,name,data.as_bytes()){
                        Ok(())=>self.output(format_args!("Saved {} ({} bytes)",name,data.len())),
                        Err(err)=>self.output(format_args!("write: {:?}",err)),
                    },
                    _=>self.say("SamenFS1 not mounted"),
                }
            },
            "rm"=>match (disk.as_mut(),fs.as_mut()){
                (Some(d),Some(f))=>match f.delete_file(d,args.trim()){
                    Ok(())=>self.output(format_args!("Removed {}",args.trim())),
                    Err(err)=>self.output(format_args!("rm: {:?}",err)),
                },
                _=>self.say("SamenFS1 not mounted"),
            },
            _=>self.output(format_args!("Unknown: {} (try help)",cmd)),
        }
    }
}
