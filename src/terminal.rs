//! Interactive PS/2-driven kernel terminal with real SamenFS1 commands.
//! Nothing is faked: ls/cat/write/rm go through the persistent ATA block device.
use core::fmt::{self,Write};
use crate::{ata::AtaDisk,kprintln,ps2::Key};
use samen_kernel::fs::{BlockDevice,Fs,FsError};

pub const MAX_LINES:usize=18;
pub const MAX_WIDTH:usize=104;
pub const INPUT_MAX:usize=100;
#[derive(Clone,Copy)]
pub struct Line {
    pub data:[u8;MAX_WIDTH],
    pub len:u8,
    pub style:u8, // 0=normal, 1=prompt, 2=success, 3=error, 4=heading
}
impl Line {
    const EMPTY:Self=Self{data:[0;MAX_WIDTH],len:0,style:0};
    pub fn value(&self)->&str {
        core::str::from_utf8(&self.data[..self.len as usize]).unwrap_or("")
    }
}
impl fmt::Write for Line {
    fn write_str(&mut self,s:&str)->fmt::Result {
        for byte in s.bytes(){
            if self.len as usize>=self.data.len(){break;}
            if byte==b'\n'||byte==b'\r'{break;}
            self.data[self.len as usize]=if byte.is_ascii(){byte}else{b'?'};
            self.len+=1;
        }
        Ok(())
    }
}
pub struct Terminal {
    pub lines:[Line;MAX_LINES],
    pub count:usize,
    pub input:[u8;INPUT_MAX],
    pub input_len:usize,
}
impl Terminal {
    pub fn new()->Self {
        let mut terminal=Self{
            lines:[Line::EMPTY;MAX_LINES],count:0,input:[0;INPUT_MAX],input_len:0
        };
        terminal.print_style("Samen Terminal  |  Rust x86-64",4);
        terminal.print_style("Persistent SamenFS1 shell - type help to begin.",0);
        terminal.print_style("Keyboard input is routed directly from PS/2.",0);
        terminal
    }
    fn push(&mut self,line:Line){
        if self.count<MAX_LINES {
            self.lines[self.count]=line;
            self.count+=1;
        } else {
            self.lines.copy_within(1..MAX_LINES,0);
            self.lines[MAX_LINES-1]=line;
        }
    }
    pub fn print_style(&mut self,text:&str,style:u8){
        let mut line=Line::EMPTY;
        line.style=style;
        let _=line.write_str(text);
        if style!=1{kprintln!("GUI-TERM: {}",line.value());}
        self.push(line);
    }
    pub fn fmt(&mut self,style:u8,args:fmt::Arguments<'_>){
        let mut line=Line::EMPTY;
        line.style=style;
        let _=line.write_fmt(args);
        if style!=1{kprintln!("GUI-TERM: {}",line.value());}
        self.push(line);
    }
    pub fn feed(&mut self,key:Key,disk:&mut Option<AtaDisk>,fs:&mut Option<Fs>)->bool{
        match key{
            Key::Backspace=> {
                if self.input_len>0{self.input_len-=1;self.input[self.input_len]=0;true}else{false}
            }
            Key::Char(c)=> {
                if c.is_ascii_graphic()||c==b' ' {
                    if self.input_len<INPUT_MAX {
                        self.input[self.input_len]=c;
                        self.input_len+=1;
                        return true;
                    }
                }
                false
            }
            Key::Enter=>{
                let mut echo=Line::EMPTY;
                echo.style=1;
                let _=echo.write_str("samen@os ~ % ");
                let _=echo.write_str(core::str::from_utf8(&self.input[..self.input_len]).unwrap_or(""));
                self.push(echo);
                // Input is a bounded stack snapshot so output may mutate logs.
                let mut temp=[0u8;INPUT_MAX];
                temp[..self.input_len].copy_from_slice(&self.input[..self.input_len]);
                let n=self.input_len;
                self.input_len=0;
                self.input.fill(0);
                if let Ok(line)=core::str::from_utf8(&temp[..n]){
                    self.execute(line.trim(),disk,fs);
                }
                true
            }
        }
    }
    fn execute(&mut self,line:&str,disk:&mut Option<AtaDisk>,fs:&mut Option<Fs>){
        if line.is_empty(){return;}
        let (command,args)=line.split_once(' ').unwrap_or((line,""));
        let args=args.trim();
        match command{
            "help"=>{
                self.print_style("help  ls  cat NAME  write NAME TEXT  rm NAME",0);
                self.print_style("stat NAME  disk  pwd  echo TEXT  clear  about",0);
                self.print_style("format YES  reboot  shutdown",0);
            }
            "clear"=>{self.count=0;}
            "pwd"=>self.print_style("/",2),
            "echo"=>self.print_style(args,0),
            "about"=>self.print_style("Samen OS | bare metal Rust | x86_64",2),
            "disk"=>{
                if let Some(d)=disk.as_ref(){
                    self.fmt(2,format_args!("Primary IDE: {} sectors | SamenFS1 mounted: {}",d.sector_count(),fs.is_some()));
                }else{self.print_style("No primary IDE disk detected.",3);}
            }
            "ls"=>{
                if let Some(tree)=fs.as_ref(){
                    let mut found=false;
                    for file in tree.files() {
                        found=true;
                        self.fmt(0,format_args!("{:>6} bytes   {}",file.size,file.name()));
                    }
                    if !found{self.print_style("Directory is empty.",0);}
                }else{self.print_style("SamenFS1 not mounted.",3);}
            }
            "stat"=>{
                if let Some(tree)=fs.as_ref() {
                    match tree.info(args) {
                        Ok(file)=>self.fmt(0,format_args!("{}: {} bytes, {} sectors",file.name(),file.size,file.sectors)),
                        Err(_)=>self.print_style("File not found.",3),
                    }
                }else{self.print_style("Filesystem unavailable.",3);}
            }
            "cat"=>{
                if let (Some(d),Some(tree))=(disk.as_mut(),fs.as_ref()){
                    let mut data=[0u8;MAX_WIDTH];
                    let mut count=0usize;
                    let result=tree.read_file(d,args,|block|{
                        let n=block.len().min(MAX_WIDTH-count);
                        data[count..count+n].copy_from_slice(&block[..n]);
                        count+=n;
                    });
                    match result{
                        Ok(())=>{
                            let s=core::str::from_utf8(&data[..count]).unwrap_or("[binary content]");
                            self.print_style(s,0);
                            if tree.info(args).is_ok_and(|f|f.size as usize>MAX_WIDTH){
                                self.print_style("(preview truncated to 104 bytes)",0);
                            }
                        }
                        Err(_)=>self.print_style("Unable to read file.",3)
                    }
                }else{self.print_style("Filesystem unavailable.",3);}
            }
            "write"=>{
                let (name,data)=args.split_once(' ').unwrap_or((args,""));
                if let (Some(d),Some(tree))=(disk.as_mut(),fs.as_mut()){
                    match tree.write_file(d,name,data.as_bytes()){
                        Ok(())=>self.fmt(2,format_args!("Saved {} ({} bytes).",name,data.len())),
                        Err(error)=>self.fmt(3,format_args!("write failed: {:?}",error)),
                    }
                }else{self.print_style("Filesystem unavailable.",3);}
            }
            "rm"=>{
                if let (Some(d),Some(tree))=(disk.as_mut(),fs.as_mut()){
                    match tree.delete_file(d,args){
                        Ok(())=>self.fmt(2,format_args!("Deleted {}.",args)),
                        Err(error)=>self.fmt(3,format_args!("rm failed: {:?}",error)),
                    }
                }else{self.print_style("Filesystem unavailable.",3);}
            }
            "format"=>{
                if args!="YES"{
                    self.print_style("Denied. Type 'format YES' to erase directory.",3);
                }else if let Some(d)=disk.as_mut(){
                    match Fs::format(d){
                        Ok(())=>{
                            *fs=Fs::mount(d).ok();
                            self.print_style("SamenFS1 formatted and mounted.",2);
                        }
                        Err(error)=>{
                            *fs=None;
                            self.fmt(3,format_args!("Format failed: {:?}",error));
                        }
                    }
                }else{self.print_style("No disk detected.",3);}
            }
            "reboot"=>crate::arch::reboot(),
            "shutdown"|"exit"=>crate::arch::poweroff(),
            _=>self.fmt(3,format_args!("Unknown command: {}  (try help)",command)),
        }
    }
}
