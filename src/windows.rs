//! Samen window manager: independent native-resolution app surfaces, a
//! proportional antialiased UI font, and functional in-kernel applications.
//! The original blue desktop and real HD dock are deliberately unchanged.
use crate::{
    gui::{App,Framebuffer},
    ui::{self,Color},
    terminal::Terminal,
};
use samen_kernel::fs::Fs;

pub const ART_W:i32=692;
pub const ART_H:i32=498;
const TITLE_H:i32=27;
const INK:Color=[31,47,73];
const MUTED:Color=[103,119,142];
const BLUE:Color=[45,123,230];
const PAPER:Color=[248,250,255];
const BORDER:Color=[214,222,235];
const WHITE:Color=[255,255,255];

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action {None,CreateSample,DeleteSelected,PreviewSelected}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rect {pub x:i32,pub y:i32,pub w:i32,pub h:i32}
impl Rect{
    pub fn contains(self,x:i32,y:i32)->bool {
        x>=self.x&&y>=self.y&&x<self.x+self.w&&y<self.y+self.h
    }
}
#[derive(Clone,Copy)]
pub struct Window {
    pub app:App,
    pub open:bool,
    pub minimized:bool,
    pub maximized:bool,
    pub rect:Rect,
    restore:Rect,
    z:u32,
}
impl Window {
    const fn new(app:App,x:i32,y:i32)->Self {
        let rect=Rect{x,y,w:474,h:302};
        Self{app,open:false,minimized:false,maximized:false,rect,restore:rect,z:0}
    }
}
#[derive(Clone,Copy)]
pub struct FileRow {name:[u8;31],pub size:u32}
impl FileRow{
    const EMPTY:Self=Self{name:[0;31],size:0};
    pub fn name(&self)->&str{
        let n=self.name.iter().position(|c|*c==0).unwrap_or(31);
        core::str::from_utf8(&self.name[..n]).unwrap_or("?")
    }
}
pub struct Manager {
    pub windows:[Window;3],
    order:u32,
    dragging:Option<(usize,i32,i32)>,
    pub files:[FileRow;24],
    pub file_count:usize,
    pub selected_file:Option<usize>,
    pub preview:[u8;256],
    pub preview_len:usize,
    pub browser_help:bool,
    pub fast_pointer:bool,
    pub terminal:Terminal,
}
impl Manager{
    pub fn new()->Self{
        Self{
            windows:[
                Window::new(App::Browser,105,73),
                Window::new(App::Files,117,76),
                Window::new(App::Terminal,128,81),
            ],
            order:1,dragging:None,
            files:[FileRow::EMPTY;24],file_count:0,
            selected_file:None,preview:[0;256],preview_len:0,
            browser_help:false,fast_pointer:false,
            terminal:Terminal::new(),
        }
    }
    fn index(&self,app:App)->usize{
        match app{App::Browser=>0,App::Files=>1,App::Terminal=>2}
    }
    fn focus(&mut self,i:usize){
        self.order=self.order.wrapping_add(1).max(1);
        self.windows[i].z=self.order;
    }
    pub fn open(&mut self,app:App){
        let i=self.index(app);
        self.windows[i].open=true;
        self.windows[i].minimized=false;
        self.focus(i);
    }
    pub fn front(&self)->Option<App>{
        self.windows.iter()
            .filter(|w|w.open&&!w.minimized)
            .max_by_key(|w|w.z).map(|w|w.app)
    }
    pub fn is_open(&self,app:App)->bool{
        let w=self.windows[self.index(app)];
        w.open&&!w.minimized
    }
    pub fn sync_files(&mut self,fs:Option<&Fs>){
        self.file_count=0;
        if let Some(fs)=fs {
            for f in fs.files().take(self.files.len()){
                self.files[self.file_count]=FileRow{name:f.name_bytes,size:f.size};
                self.file_count+=1;
            }
        }
        if self.selected_file.is_some_and(|i|i>=self.file_count){
            self.selected_file=None;
            self.preview_len=0;
        }
    }
    pub fn file_name(&self)->Option<&str>{
        let i=self.selected_file?;
        if i<self.file_count{Some(self.files[i].name())}else{None}
    }
    pub fn set_preview(&mut self,bytes:&[u8]){
        self.preview_len=bytes.len().min(self.preview.len());
        self.preview[..self.preview_len].copy_from_slice(&bytes[..self.preview_len]);
    }
    pub fn clear_preview(&mut self){self.preview_len=0;}
    pub fn on_move(&mut self,x:i32,y:i32,down:bool){
        if !down{self.dragging=None;return;}
        let Some((i,dx,dy))=self.dragging else{return};
        let w=&mut self.windows[i];
        if !w.maximized{
            w.rect.x=(x-dx).clamp(-w.rect.w+80,ART_W-80);
            w.rect.y=(y-dy).clamp(0,ART_H-70);
        }
    }
    pub fn release(&mut self){self.dragging=None;}
    pub fn is_dragging(&self)->bool{self.dragging.is_some()}
    pub fn click(&mut self,x:i32,y:i32)->Action{
        let mut target=None;
        let mut highest=0;
        for i in 0..3{
            let w=self.windows[i];
            if w.open&&!w.minimized&&w.rect.contains(x,y)&&w.z>=highest{
                target=Some(i);
                highest=w.z;
            }
        }
        let Some(i)=target else{return Action::None};
        self.focus(i);
        let r=self.windows[i].rect;
        let relx=x-r.x;
        let rely=y-r.y;
        if rely<TITLE_H{
            if relx>=r.w-31{
                self.windows[i].open=false;
                self.dragging=None;
            }else if relx>=r.w-57{
                self.windows[i].minimized=true;
                self.dragging=None;
            }else if relx>=r.w-84{
                let w=&mut self.windows[i];
                if w.maximized{w.rect=w.restore;}else{
                    w.restore=w.rect;
                    w.rect=Rect{x:14,y:21,w:ART_W-28,h:ART_H-105};
                }
                w.maximized=!w.maximized;
                self.dragging=None;
            }else{
                self.dragging=Some((i,relx,rely));
            }
            return Action::None;
        }
        match self.windows[i].app{
            App::Browser=>{
                if rely<70{
                    if relx<90{self.browser_help=false;}
                    else if relx<177{self.browser_help=true;}
                }
            }
            App::Files=>{
                if rely>=34&&rely<67{
                    if relx<117{return Action::CreateSample;}
                    if relx<214{return Action::DeleteSelected;}
                }
                if rely>=93{
                    let idx=((rely-93)/24) as usize;
                    if idx<self.file_count&&idx<7{
                        self.selected_file=Some(idx);
                        self.preview_len=0;
                        return Action::PreviewSelected;
                    }
                }
            }
            App::Terminal=>{}
        }
        Action::None
    }
    pub fn draw(&self,fb:&mut Framebuffer){
        let mut painted=[false;3];
        for _ in 0..3{
            let mut candidate=None;
            for i in 0..3{
                let w=self.windows[i];
                if !painted[i]&&w.open&&!w.minimized
                    &&candidate.is_none_or(|j:usize|w.z<self.windows[j].z){
                    candidate=Some(i);
                }
            }
            if let Some(i)=candidate{
                self.draw_window(fb,self.windows[i]);
                painted[i]=true;
            }
        }
    }
    fn draw_window(&self,fb:&mut Framebuffer,w:Window){
        let r=w.rect;
        let dark=w.app==App::Terminal;
        // A real layered shadow, then two anti-aliased native-resolution
        // surfaces. No output passes through the old logical_blend grid.
        ui::round(fb,Rect{x:r.x+7,y:r.y+11,w:r.w,h:r.h},15.0,[2,23,58],47);
        ui::round(fb,Rect{x:r.x+3,y:r.y+5,w:r.w,h:r.h},14.0,[7,27,58],39);
        ui::round(fb,r,13.5,if dark{[14,23,43]}else{WHITE},255);
        ui::round(fb,Rect{x:r.x+1,y:r.y+1,w:r.w-2,h:r.h-2},12.5,
            if dark{[22,33,54]}else{PAPER},255);

        // Window chrome remains deliberately in the old click positions.
        ui::bar(fb,Rect{x:r.x+10,y:r.y+TITLE_H-1,w:r.w-20,h:1},
            if dark{[49,62,88]}else{BORDER},185);
        let title_color=if dark{[238,245,255]}else{INK};
        ui::text(fb,r.x+15,r.y+4,w.app.label(),title_color,0.83,150);
        let controls=[[244,91,98],[244,188,65],[72,205,128]];
        for (idx,color) in controls.iter().enumerate(){
            let cx=r.x+r.w-20-idx as i32*26;
            ui::circle(fb,cx,r.y+13,6,*color,255);
            ui::circle(fb,cx-1,r.y+11,2,[255,255,255],79);
        }
        match w.app{
            App::Browser=>self.draw_browser(fb,r),
            App::Files=>self.draw_files(fb,r),
            App::Terminal=>self.draw_terminal(fb,r),
        }
    }
    fn draw_browser(&self,fb:&mut Framebuffer,r:Rect){
        let ax=r.x;
        let ay=r.y;
        // Compact real toolbar and address bar: no fake internet capability.
        ui::round(fb,Rect{x:ax+13,y:ay+35,w:74,h:29},8.0,
            if !self.browser_help{[220,234,254]}else{[238,241,247]},255);
        ui::text(fb,ax+25,ay+41,"Home",INK,0.78,62);
        ui::round(fb,Rect{x:ax+94,y:ay+35,w:74,h:29},8.0,
            if self.browser_help{[220,234,254]}else{[238,241,247]},255);
        ui::text(fb,ax+106,ay+41,"Help",INK,0.78,58);
        ui::round(fb,Rect{x:ax+177,y:ay+35,w:r.w-190,h:29},8.0,[234,239,248],255);
        ui::circle(fb,ax+190,ay+50,4,[49,189,139],255);
        ui::text(fb,ax+199,ay+41,
            if self.browser_help{"samen://help"}else{"samen://home"},MUTED,0.77,r.w-224);
        ui::bar(fb,Rect{x:ax+15,y:ay+73,w:r.w-30,h:1},BORDER,151);

        // First-party local content with breathing room and hierarchy.
        ui::round(fb,Rect{x:ax+19,y:ay+85,w:r.w-38,h:r.h-102},14.0,[239,245,254],255);
        ui::round(fb,Rect{x:ax+33,y:ay+101,w:58,h:24},12.0,[219,232,251],255);
        ui::text(fb,ax+42,ay+103,"SAMEN",BLUE,0.72,53);
        if self.browser_help{
            ui::text(fb,ax+34,ay+133,"Getting started",INK,1.43,r.w-72);
            ui::text(fb,ax+35,ay+172,"Home opens the local start page.",MUTED,0.86,r.w-72);
            ui::text(fb,ax+35,ay+195,"Files manages your SamenFS1 disk.",MUTED,0.86,r.w-72);
            ui::text(fb,ax+35,ay+218,"Terminal runs real kernel commands.",MUTED,0.86,r.w-72);
        }else{
            ui::text(fb,ax+34,ay+130,"Welcome to Samen",INK,1.43,r.w-70);
            ui::text(fb,ax+35,ay+171,"Your desktop. Your operating system.",MUTED,0.88,r.w-72);
            ui::round(fb,Rect{x:ax+35,y:ay+206,w:163,h:34},9.0,BLUE,255);
            ui::text(fb,ax+48,ay+213,"Explore local pages",WHITE,0.77,139);
            ui::text(fb,ax+212,ay+212,"Offline preview",MUTED,0.78,170);
        }
        ui::text(fb,ax+22,ay+r.h-22,
            "Local pages only  |  Internet engine not installed",MUTED,0.70,r.w-45);
    }
    fn draw_files(&self,fb:&mut Framebuffer,r:Rect){
        let x=r.x;
        let y=r.y;
        // Toolbar aligns with the existing working file-action hit boxes.
        ui::round(fb,Rect{x:x+15,y:y+35,w:103,h:30},8.0,BLUE,255);
        ui::text(fb,x+24,y+41,"+ New file",WHITE,0.80,90);
        ui::round(fb,Rect{x:x+125,y:y+35,w:82,h:30},8.0,[231,237,246],255);
        ui::text(fb,x+139,y+41,"Delete",INK,0.80,65);
        ui::round(fb,Rect{x:x+221,y:y+35,w:r.w-235,h:30},8.0,[241,245,251],255);
        ui::text(fb,x+236,y+41,"SamenFS1 /",MUTED,0.78,r.w-260);
        ui::bar(fb,Rect{x:x+16,y:y+79,w:r.w-32,h:1},BORDER,192);
        ui::text(fb,x+27,y+80,"Name",MUTED,0.74,148);
        ui::text(fb,x+r.w-111,y+80,"Size",MUTED,0.74,75);
        if self.file_count==0{
            ui::circle(fb,x+r.w/2,y+141,27,[222,235,250],255);
            ui::text(fb,x+r.w/2-42,y+131,"Empty folder",INK,0.90,120);
            ui::text(fb,x+r.w/2-88,y+172,"Create a file to get started.",MUTED,0.79,210);
        }
        for i in 0..self.file_count.min(7){
            let row=y+93+i as i32*24;
            if self.selected_file==Some(i){
                ui::round(fb,Rect{x:x+15,y:row-2,w:r.w-30,h:23},7.0,[216,231,251],255);
            }else if i%2==1 {
                ui::round(fb,Rect{x:x+16,y:row-2,w:r.w-32,h:23},5.0,[243,247,252],198);
            }
            ui::round(fb,Rect{x:x+26,y:row+3,w:14,h:16},3.0,[124,177,240],255);
            ui::round(fb,Rect{x:x+28,y:row+6,w:10,h:2},1.0,[246,252,255],210);
            ui::text(fb,x+47,row+1,self.files[i].name(),INK,0.80,r.w-185);
            ui::number(fb,x+r.w-112,row+1,self.files[i].size,MUTED,0.75,92);
        }
        ui::bar(fb,Rect{x:x+17,y:y+r.h-48,w:r.w-34,h:1},BORDER,173);
        if let Some(i)=self.selected_file {
            if i<self.file_count {
                ui::text(fb,x+24,y+r.h-43,self.files[i].name(),BLUE,0.78,r.w-48);
                let n=self.preview_len.min(70);
                let value=core::str::from_utf8(&self.preview[..n]).unwrap_or("[binary file]");
                ui::text(fb,x+24,y+r.h-21,value,MUTED,0.72,r.w-48);
            }
        }else{
            ui::text(fb,x+24,y+r.h-32,
                "Select a file to preview its contents",MUTED,0.75,r.w-46);
        }
    }
    fn draw_terminal(&self,fb:&mut Framebuffer,r:Rect){
        let x=r.x;
        let y=r.y;
        ui::round(fb,Rect{x:x+9,y:y+35,w:r.w-18,h:r.h-45},9.0,[9,17,33],255);
        ui::circle(fb,x+23,y+48,3,[81,217,152],255);
        ui::text(fb,x+34,y+40,"samen@kernel  /  shell", [164,183,211],0.74,r.w-65);
        ui::bar(fb,Rect{x:x+22,y:y+70,w:r.w-44,h:1},[46,60,83],219);
        let visible=8usize.min(self.terminal.count);
        let skip=self.terminal.count.saturating_sub(visible);
        for (i,line) in self.terminal.lines().skip(skip).enumerate(){
            let line_y=y+76+i as i32*20;
            let color=if line.starts_with("samen:~$"){[124,219,173]}
                else{[212,223,242]};
            ui::text(fb,x+25,line_y,line,color,0.72,r.w-53);
        }
        let prompt_y=y+r.h-35;
        ui::text(fb,x+25,prompt_y,"$ ",[126,224,177],0.83,27);
        ui::text(fb,x+42,prompt_y,self.terminal.command_line(),
            [242,247,255],0.82,r.w-73);
        // A real caret follows keyboard entry; input is never just a mockup.
        let cursor_x=x+43+((self.terminal.input_len as i32)*9).min(r.w-86);
        ui::bar(fb,Rect{x:cursor_x,y:prompt_y+3,w:1,h:14},[116,215,173],221);
    }
}
