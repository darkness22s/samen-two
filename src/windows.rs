//! Small fixed-capacity desktop window manager for Samen's framebuffer.
//! All coordinates are in the 692x498 Figma artwork's coordinate space.
use crate::gui::{App, Framebuffer};
use samen_kernel::fs::Fs;

pub const ART_W: i32 = 692;
pub const ART_H: i32 = 498;
const TITLE_H: i32 = 27;
const TEXT: [u8;3] = [27,40,68];
const BLUE: [u8;3] = [31,111,213];

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action { None, CreateSample, DeleteSelected, PreviewSelected }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {pub x:i32,pub y:i32,pub w:i32,pub h:i32}
impl Rect {
    pub fn contains(self,x:i32,y:i32)->bool {
        x>=self.x && y>=self.y && x<self.x+self.w && y<self.y+self.h
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
    const fn new(app:App, x:i32,y:i32)->Self{
        let rect=Rect{x,y,w:474,h:302};
        Self{app,open:false,minimized:false,maximized:false,rect,restore:rect,z:0}
    }
}
#[derive(Clone,Copy)]
pub struct FileRow {name:[u8;31],pub size:u32}
impl FileRow {
    const EMPTY:Self=Self{name:[0;31],size:0};
    pub fn name(&self)->&str {
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
    pub glass:bool,
    pub fast_pointer:bool,
    pub purple_accent:bool,
    pub browser_help:bool,
}
impl Manager {
    pub fn new()->Self {
        Self {
            windows:[Window::new(App::Browser,105,73),Window::new(App::Files,117,76),
                     Window::new(App::Settings,128,81)],
            order:1,dragging:None,files:[FileRow::EMPTY;24],file_count:0,
            selected_file:None,preview:[0;256],preview_len:0,glass:true,
            fast_pointer:false,purple_accent:false,browser_help:false,
        }
    }
    pub fn sync_files(&mut self,fs:Option<&Fs>) {
        self.file_count=0;
        if let Some(fs)=fs {
            for f in fs.files().take(self.files.len()) {
                self.files[self.file_count]=FileRow{name:f.name_bytes,size:f.size};
                self.file_count+=1;
            }
        }
        if self.selected_file.is_some_and(|i|i>=self.file_count) {
            self.selected_file=None;
            self.preview_len=0;
        }
    }
    pub fn open(&mut self,app:App) {
        let i=self.index(app);
        self.windows[i].open=true;
        self.windows[i].minimized=false;
        self.focus(i);
    }
    fn index(&self,app:App)->usize {match app{App::Browser=>0,App::Files=>1,App::Settings=>2}}
    fn focus(&mut self,i:usize){
        self.order=self.order.wrapping_add(1).max(1);
        self.windows[i].z=self.order;
    }
    pub fn is_open(&self,app:App)->bool {let w=&self.windows[self.index(app)];w.open&&!w.minimized}
    pub fn front(&self)->Option<App>{
        self.windows.iter().filter(|w|w.open&&!w.minimized).max_by_key(|w|w.z).map(|w|w.app)
    }
    pub fn title_position(&self,app:App)->Rect{self.windows[self.index(app)].rect}
    pub fn file_name(&self)->Option<&str>{
        let i=self.selected_file?;
        if i<self.file_count{Some(self.files[i].name())}else{None}
    }
    pub fn set_preview(&mut self,bytes:&[u8]) {
        self.preview_len=bytes.len().min(self.preview.len());
        self.preview[..self.preview_len].copy_from_slice(&bytes[..self.preview_len]);
    }
    pub fn clear_preview(&mut self){self.preview_len=0;}
    pub fn on_move(&mut self,x:i32,y:i32,down:bool){
        if !down {self.dragging=None;return;}
        let Some((i,dx,dy))=self.dragging else{return};
        let w=&mut self.windows[i];
        if !w.maximized {
            w.rect.x=(x-dx).clamp(-w.rect.w+70,ART_W-70);
            w.rect.y=(y-dy).clamp(0,ART_H-70);
        }
    }
    pub fn click(&mut self,x:i32,y:i32)->Action {
        // Topmost window first: every window can be focused, dragged or closed.
        let mut target=None;
        let mut top=0;
        for i in 0..3 {
            let w=self.windows[i];
            if w.open&&!w.minimized&&w.rect.contains(x,y)&&w.z>=top {
                top=w.z;target=Some(i);
            }
        }
        let Some(i)=target else{return Action::None};
        self.focus(i);
        let r=self.windows[i].rect;
        let relx=x-r.x;
        let rely=y-r.y;
        if rely<TITLE_H {
            if relx>=r.w-31 {
                self.windows[i].open=false;
                self.dragging=None;
            } else if relx>=r.w-57 {
                self.windows[i].minimized=true;
                self.dragging=None;
            } else if relx>=r.w-84 {
                let w=&mut self.windows[i];
                if w.maximized {w.rect=w.restore;} else {
                    w.restore=w.rect;
                    w.rect=Rect{x:14,y:21,w:ART_W-28,h:ART_H-105};
                }
                w.maximized=!w.maximized;
                self.dragging=None;
            } else {
                self.dragging=Some((i,relx,rely));
            }
            return Action::None;
        }
        match self.windows[i].app {
            App::Browser=>{
                // Home and help are local pages; networking/HTML are not implemented.
                if rely<64 {
                    if relx<84 {self.browser_help=false;}
                    else if relx<165 {self.browser_help=true;}
                }
            }
            App::Settings=>{
                if rely>=91&&rely<137 {self.glass=!self.glass;}
                else if rely>=153&&rely<200 {self.fast_pointer=!self.fast_pointer;}
                else if rely>=218&&rely<266 {self.purple_accent=!self.purple_accent;}
            }
            App::Files=>{
                if rely>=34 && rely<67 {
                    if relx<117{return Action::CreateSample;}
                    if relx<214{return Action::DeleteSelected;}
                }
                if rely>=93 {
                    let index=((rely-93)/24) as usize;
                    if index<self.file_count && index<7 {
                        self.selected_file=Some(index);
                        self.preview_len=0;
                        return Action::PreviewSelected;
                    }
                }
            }
        }
        Action::None
    }
    pub fn release(&mut self){self.dragging=None;}
    pub fn is_dragging(&self)->bool{self.dragging.is_some()}
    pub fn draw(&self,fb:&mut Framebuffer){
        let mut painted=[false;3];
        for _ in 0..3 {
            let mut candidate=None;
            for i in 0..3 {
                let w=&self.windows[i];
                if !painted[i]&&w.open&&!w.minimized&&candidate.is_none_or(|j:usize| w.z<self.windows[j].z){
                    candidate=Some(i);
                }
            }
            if let Some(i)=candidate {self.draw_window(fb,self.windows[i]);painted[i]=true;}
        }
    }
    fn accent(&self)->[u8;3]{if self.purple_accent{[116,87,201]}else{BLUE}}
    fn draw_window(&self,fb:&mut Framebuffer,w:Window) {
        let r=w.rect;
        // Soft shadow, rounded light glass panels and a light toolbar.
        rounded(fb,Rect{x:r.x+5,y:r.y+7,w:r.w,h:r.h},14,[9,25,55],78);
        rounded(fb,r,13,[248,249,252],if self.glass{234}else{255});
        rect(fb,Rect{x:r.x+1,y:r.y+TITLE_H,w:r.w-2,h:1},[194,203,217],255);
        text(fb,r.x+15,r.y+10,w.app.label(),TEXT,1);
        for (idx,color) in [[245,97,93],[247,190,67],[76,201,110]].iter().enumerate(){
            // Window controls are deliberately modest and work from the pointer.
            let x=r.x+r.w-20-idx as i32*26;
            circle(fb,x,r.y+13,7,*color);
        }
        match w.app {
            App::Browser=>self.draw_browser(fb,r),
            App::Files=>self.draw_files(fb,r),
            App::Settings=>self.draw_settings(fb,r),
        }
    }
    fn draw_browser(&self,fb:&mut Framebuffer,r:Rect){
        let a=self.accent();
        rounded(fb,Rect{x:r.x+13,y:r.y+35,w:75,h:25},7,[229,235,246],255);
        text(fb,r.x+23,r.y+44,"HOME",TEXT,1);
        rounded(fb,Rect{x:r.x+94,y:r.y+35,w:74,h:25},7,[229,235,246],255);
        text(fb,r.x+105,r.y+44,"HELP",TEXT,1);
        rounded(fb,Rect{x:r.x+177,y:r.y+35,w:r.w-190,h:25},7,[235,239,246],255);
        text(fb,r.x+191,r.y+44,if self.browser_help{"about:help"}else{"about:samen"},[84,93,112],1);
        rect(fb,Rect{x:r.x+13,y:r.y+72,w:r.w-26,h:1},[205,212,224],255);
        text(fb,r.x+27,r.y+94,if self.browser_help{"BROWSER HELP"}else{"SAMEN BROWSER"},a,2);
        text(fb,r.x+27,r.y+131,if self.browser_help{"HOME RETURNS TO THE START PAGE."}else{"WELCOME TO SAMEN."},TEXT,1);
        text(fb,r.x+27,r.y+156,if self.browser_help{"REAL WEB SUPPORT REQUIRES NETWORKING."}else{"THE WINDOW IS INTERACTIVE."},TEXT,1);
        text(fb,r.x+27,r.y+181,"NO HTTP OR HTML ENGINE INSTALLED YET.",[87,102,128],1);
        text(fb,r.x+27,r.y+235,"SAMEN OS  /  LOCAL PAGE",a,1);
    }
    fn draw_settings(&self,fb:&mut Framebuffer,r:Rect) {
        let accent=self.accent();
        text(fb,r.x+20,r.y+48,"APPEARANCE & INPUT",accent,2);
        for (idx,(name,description,on)) in [
            ("GLASS WINDOWS","TRANSLUCENT WINDOW PANELS",self.glass),
            ("POINTER SPEED","DOUBLE CURSOR SPEED",self.fast_pointer),
            ("PURPLE ACCENT","SWITCH WINDOW ACCENT",self.purple_accent),
        ].iter().enumerate() {
            let y=r.y+92+idx as i32*63;
            rounded(fb,Rect{x:r.x+17,y:y-9,w:r.w-34,h:52},9,[236,240,248],236);
            text(fb,r.x+28,y+1,name,TEXT,1);
            text(fb,r.x+28,y+19,description,[105,117,136],1);
            rounded(fb,Rect{x:r.x+r.w-73,y:y+4,w:43,h:23},12,if *on {accent}else{[164,175,190]},255);
            circle(fb,if *on {r.x+r.w-43}else{r.x+r.w-62},y+15,8,[255,255,255]);
        }
    }
    fn draw_files(&self,fb:&mut Framebuffer,r:Rect) {
        let accent=self.accent();
        rounded(fb,Rect{x:r.x+15,y:r.y+35,w:103,h:29},8,accent,255);
        text(fb,r.x+25,r.y+46,"NEW SAMPLE",[255,255,255],1);
        rounded(fb,Rect{x:r.x+125,y:r.y+35,w:82,h:29},8,[229,235,245],255);
        text(fb,r.x+137,r.y+46,"DELETE",TEXT,1);
        text(fb,r.x+224,r.y+46,"SAMENFS1",TEXT,1);
        rect(fb,Rect{x:r.x+14,y:r.y+82,w:r.w-28,h:1},[205,211,221],255);
        text(fb,r.x+22,r.y+79,"NAME",TEXT,1);
        text(fb,r.x+r.w-115,r.y+79,"BYTES",TEXT,1);
        if self.file_count==0 {
            text(fb,r.x+30,r.y+115,"NO FILES. CLICK NEW SAMPLE.",[99,112,132],1);
        }
        for i in 0..self.file_count.min(7) {
            let y=r.y+94+i as i32*24;
            if self.selected_file==Some(i) {
                rounded(fb,Rect{x:r.x+16,y:y-3,w:r.w-32,h:22},5,[210,229,250],245);
            }
            text(fb,r.x+26,y+5,self.files[i].name(),TEXT,1);
            number(fb,r.x+r.w-113,y+5,self.files[i].size,TEXT);
        }
        rect(fb,Rect{x:r.x+14,y:r.y+r.h-45,w:r.w-28,h:1},[211,219,230],255);
        if let Some(i)=self.selected_file {
            if i<self.file_count{
                text(fb,r.x+23,r.y+r.h-32,self.files[i].name(),accent,1);
                let n=self.preview_len.min(42);
                let chars=core::str::from_utf8(&self.preview[..n]).unwrap_or("[BINARY FILE]");
                text(fb,r.x+23,r.y+r.h-17,chars,[87,102,124],1);
            }
        }else {
            text(fb,r.x+23,r.y+r.h-27,"SELECT A FILE TO PREVIEW IT.",[99,112,132],1);
        }
    }
}

fn number(fb:&mut Framebuffer,x:i32,y:i32,n:u32,color:[u8;3]){
    let mut buf=[0u8;10];let mut pos=buf.len();let mut n=n;
    loop{pos-=1;buf[pos]=b'0'+(n%10) as u8;n/=10;if n==0{break;}}
    if let Ok(s)=core::str::from_utf8(&buf[pos..]){text(fb,x,y,s,color,1);}
}
fn rect(fb:&mut Framebuffer,r:Rect,c:[u8;3],alpha:u8){
    if r.w<=0||r.h<=0{return;}
    for y in r.y.max(0)..(r.y+r.h).min(ART_H) {
        for x in r.x.max(0)..(r.x+r.w).min(ART_W) {
            fb.logical_blend(x,y,c,alpha);
        }
    }
}
fn rounded(fb:&mut Framebuffer,r:Rect,radius:i32,c:[u8;3],alpha:u8){
    if r.w<=0||r.h<=0{return;}
    let radius=radius.min(r.w/2).min(r.h/2);
    for y in r.y.max(0)..(r.y+r.h).min(ART_H) {
        for x in r.x.max(0)..(r.x+r.w).min(ART_W) {
            let dx=if x<r.x+radius {r.x+radius-x}else if x>=r.x+r.w-radius {x-(r.x+r.w-radius-1)}else{0};
            let dy=if y<r.y+radius {r.y+radius-y}else if y>=r.y+r.h-radius {y-(r.y+r.h-radius-1)}else{0};
            if dx*dx+dy*dy<=radius*radius{fb.logical_blend(x,y,c,alpha);}
        }
    }
}
fn circle(fb:&mut Framebuffer,cx:i32,cy:i32,r:i32,color:[u8;3]){
    for y in -r..=r {for x in -r..=r {
        if x*x+y*y<=r*r {fb.logical_blend(cx+x,cy+y,color,255);}
    }}
}
fn text(fb:&mut Framebuffer,x:i32,y:i32,value:&str,color:[u8;3],size:i32){
    let mut pen=x;
    for b in value.bytes().take(69) {
        let glyph=glyph(b.to_ascii_uppercase());
        for (dy,row) in glyph.iter().enumerate() {
            for dx in 0..5 {
                if row & (1<<(4-dx))!=0 {
                    for yy in 0..size {for xx in 0..size {
                        fb.logical_blend(pen+dx as i32*size+xx,y+dy as i32*size+yy,color,255);
                    }}
                }
            }
        }
        pen+=6*size;
    }
}
// Compact 5x7 glyphs. Text rendering uses no heap or external font subsystem.
fn glyph(c:u8)->[u8;7] {
    match c {
        b'A'=>[14,17,17,31,17,17,17],b'B'=>[30,17,17,30,17,17,30],
        b'C'=>[14,17,16,16,16,17,14],b'D'=>[30,17,17,17,17,17,30],
        b'E'=>[31,16,16,30,16,16,31],b'F'=>[31,16,16,30,16,16,16],
        b'G'=>[14,17,16,23,17,17,15],b'H'=>[17,17,17,31,17,17,17],
        b'I'=>[31,4,4,4,4,4,31],b'J'=>[7,2,2,2,18,18,12],
        b'K'=>[17,18,20,24,20,18,17],b'L'=>[16,16,16,16,16,16,31],
        b'M'=>[17,27,21,21,17,17,17],b'N'=>[17,25,21,19,17,17,17],
        b'O'=>[14,17,17,17,17,17,14],b'P'=>[30,17,17,30,16,16,16],
        b'Q'=>[14,17,17,17,21,18,13],b'R'=>[30,17,17,30,20,18,17],
        b'S'=>[15,16,16,14,1,1,30],b'T'=>[31,4,4,4,4,4,4],
        b'U'=>[17,17,17,17,17,17,14],b'V'=>[17,17,17,17,17,10,4],
        b'W'=>[17,17,17,21,21,21,10],b'X'=>[17,17,10,4,10,17,17],
        b'Y'=>[17,17,10,4,4,4,4],b'Z'=>[31,1,2,4,8,16,31],
        b'0'=>[14,17,19,21,25,17,14],b'1'=>[4,12,4,4,4,4,14],
        b'2'=>[14,17,1,2,4,8,31],b'3'=>[30,1,1,14,1,1,30],
        b'4'=>[2,6,10,18,31,2,2],b'5'=>[31,16,30,1,1,17,14],
        b'6'=>[6,8,16,30,17,17,14],b'7'=>[31,1,2,4,8,8,8],
        b'8'=>[14,17,17,14,17,17,14],b'9'=>[14,17,17,15,1,2,12],
        b'.'=>[0,0,0,0,0,12,12],b':'=>[0,12,12,0,12,12,0],
        b'/'=>[1,1,2,4,8,16,16],b'-'=>[0,0,0,31,0,0,0],
        b'_'=>[0,0,0,0,0,0,31],b'('=>[2,4,8,8,8,4,2],
        b')'=>[8,4,2,2,2,4,8],b'!'=>[4,4,4,4,4,0,4],
        b'?'=>[14,17,1,2,4,0,4],b'='=>[0,31,0,31,0,0,0],
        b' '=>[0;7],_=>[0,0,0,0,0,0,0],
    }
}
