//! Native-resolution Samen application windows. Geometry is specified in
//! Figma's logical layout coordinates, then rasterized at the REAL screen
//! resolution with smooth primitives and antialiased Inter-derived text.
use crate::gui::{App,Framebuffer};
use crate::terminal::Terminal;
use crate::ui::Ui;
use samen_kernel::fs::Fs;

pub const ART_W:i32=692;
pub const ART_H:i32=498;
const TITLE_H:i32=29;
type Color=[u8;3];
const INK:Color=[36,54,76];
const MUTED:Color=[111,129,152];
const BLUE:Color=[48,116,208];
const PALE:Color=[236,244,254];
const HAIRLINE:Color=[213,224,237];

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{None,CreateSample,DeleteSelected,PreviewSelected}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rect{pub x:i32,pub y:i32,pub w:i32,pub h:i32}
impl Rect{
    pub fn contains(self,x:i32,y:i32)->bool{
        x>=self.x&&y>=self.y&&x<self.x+self.w&&y<self.y+self.h
    }
}
#[derive(Clone,Copy)]
pub struct Window{
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
pub struct FileRow{pub name:[u8;31],pub size:u32}
impl FileRow{
    const EMPTY:Self=Self{name:[0;31],size:0};
    pub fn name(&self)->&str{
        let n=self.name.iter().position(|c|*c==0).unwrap_or(31);
        core::str::from_utf8(&self.name[..n]).unwrap_or("?")
    }
}
pub struct Manager{
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
impl Manager {
    pub fn new()->Self{
        Self{
            windows:[
                Window::new(App::Browser,105,73),
                Window::new(App::Files,117,76),
                Window::new(App::Terminal,128,81),
            ],
            order:1,dragging:None,files:[FileRow::EMPTY;24],file_count:0,
            selected_file:None,preview:[0;256],preview_len:0,
            browser_help:false,fast_pointer:false,terminal:Terminal::new(),
        }
    }
    pub fn sync_files(&mut self,fs:Option<&Fs>){
        self.file_count=0;
        if let Some(fs)=fs {
            for entry in fs.files().take(self.files.len()){
                self.files[self.file_count]=FileRow{name:entry.name_bytes,size:entry.size};
                self.file_count+=1;
            }
        }
        if self.selected_file.is_some_and(|i|i>=self.file_count){
            self.selected_file=None;
            self.preview_len=0;
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
    pub fn is_open(&self,app:App)->bool{
        let w=&self.windows[self.index(app)];
        w.open&&!w.minimized
    }
    pub fn front(&self)->Option<App>{
        self.windows.iter()
            .filter(|w|w.open&&!w.minimized)
            .max_by_key(|w|w.z)
            .map(|w|w.app)
    }
    pub fn terminal_focused(&self)->bool{self.front()==Some(App::Terminal)}
    pub fn title_position(&self,app:App)->Rect{self.windows[self.index(app)].rect}
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
        if !down {self.dragging=None;return;}
        let Some((i,dx,dy))=self.dragging else{return};
        let w=&mut self.windows[i];
        if !w.maximized{
            w.rect.x=(x-dx).clamp(-w.rect.w+75,ART_W-75);
            w.rect.y=(y-dy).clamp(0,ART_H-72);
        }
    }
    pub fn click(&mut self,x:i32,y:i32)->Action{
        let mut target=None;let mut top=0;
        for i in 0..3{
            let w=self.windows[i];
            if w.open&&!w.minimized&&w.rect.contains(x,y)&&w.z>=top{
                top=w.z;target=Some(i);
            }
        }
        let Some(i)=target else{return Action::None};
        self.focus(i);
        let r=self.windows[i].rect;
        let relx=x-r.x;let rely=y-r.y;
        if rely<TITLE_H{
            if relx>=r.w-31{
                self.windows[i].open=false;self.dragging=None;
            }else if relx>=r.w-57{
                self.windows[i].minimized=true;self.dragging=None;
            }else if relx>=r.w-84{
                let w=&mut self.windows[i];
                if w.maximized{w.rect=w.restore;}else{
                    w.restore=w.rect;
                    w.rect=Rect{x:14,y:21,w:ART_W-28,h:ART_H-105};
                }
                w.maximized=!w.maximized;self.dragging=None;
            }else{
                self.dragging=Some((i,relx,rely));
            }
            return Action::None;
        }
        match self.windows[i].app{
            App::Browser=>{
                if (34..70).contains(&rely){
                    if (12..93).contains(&relx){self.browser_help=false;}
                    if (93..175).contains(&relx){self.browser_help=true;}
                }
            }
            App::Terminal=>{}
            App::Files=>{
                if (34..72).contains(&rely){
                    if (13..120).contains(&relx){return Action::CreateSample;}
                    if (120..215).contains(&relx){return Action::DeleteSelected;}
                }
                if (93..265).contains(&rely)&&relx>=119{
                    let index=((rely-93)/24) as usize;
                    if index<self.file_count&&index<7{
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
        let mut drawn=[false;3];
        for _ in 0..3{
            let mut candidate=None;
            for i in 0..3{
                let w=&self.windows[i];
                if !drawn[i]&&w.open&&!w.minimized
                   &&candidate.is_none_or(|j:usize|w.z<self.windows[j].z){
                    candidate=Some(i);
                }
            }
            if let Some(i)=candidate{
                self.draw_window(fb,self.windows[i]);
                drawn[i]=true;
            }
        }
    }
    fn draw_window(&self,fb:&mut Framebuffer,w:Window){
        let mut ui=Ui::new(fb);
        let r=w.rect;
        let (x,y,ww,hh)=(r.x as f32,r.y as f32,r.w as f32,r.h as f32);
        let is_terminal=w.app==App::Terminal;
        ui.round(x+4.5,y+9.0,ww,hh,13.7,[3,22,50],45);
        ui.round(x+2.0,y+4.5,ww,hh,14.0,[17,40,80],28);
        ui.round(x,y,ww,hh,13.4,if is_terminal{[21,31,47]}else{[250,252,255]},255);
        ui.round(x+0.7,y+0.7,ww-1.4,hh-1.4,12.7,
                 if is_terminal{[23,35,52]}else{[248,251,255]},255);
        // Top inset title bar and hairline reinforce the bright glass style.
        ui.round(x+2.0,y+2.0,ww-4.0,30.0,11.0,
                 if is_terminal{[28,42,60]}else{[244,248,253]},255);
        ui.rect(x+1.0,y+28.2,ww-2.0,3.5,
                if is_terminal{[28,42,60]}else{[244,248,253]},255);
        ui.rect(x+13.0,y+31.0,ww-26.0,0.6,
                if is_terminal{[75,96,121]}else{HAIRLINE},170);
        ui.text(x+17.0,y+20.5,12.2,true,w.app.label(),
                if is_terminal{[226,235,248]}else{INK},ww-127.0);
        let dot_colors:[[u8;3];3]=[
            [241,99,100],[247,189,86],[71,197,128]
        ];
        for (n,color) in dot_colors.iter().enumerate(){
            let cx=x+ww-20.0-n as f32*26.0;
            ui.circle(cx,y+13.5,6.7,*color,255);
            ui.circle(cx-1.8,y+11.6,1.4,[255,255,255],76);
        }
        match w.app{
            App::Browser=>self.draw_browser(&mut ui,r),
            App::Files=>self.draw_files(&mut ui,r),
            App::Terminal=>self.draw_terminal(&mut ui,r),
        }
    }
    fn draw_browser(&self,ui:&mut Ui<'_>,r:Rect){
        let (x,y,w,h)=(r.x as f32,r.y as f32,r.w as f32,r.h as f32);
        // Native toolbar with functional Home and Help tabs.
        ui.round(x+14.0,y+38.0,74.0,30.0,9.0,
                 if !self.browser_help{[215,231,255]}else{[239,244,250]},255);
        ui.text(x+29.0,y+57.7,11.0,true,"Home",
                if !self.browser_help{BLUE}else{MUTED},51.0);
        ui.round(x+94.0,y+38.0,74.0,30.0,9.0,
                 if self.browser_help{[215,231,255]}else{[239,244,250]},255);
        ui.text(x+110.0,y+57.7,11.0,true,"Guide",
                if self.browser_help{BLUE}else{MUTED},51.0);
        ui.round(x+177.0,y+38.0,w-193.0,30.0,9.0,[238,244,250],255);
        ui.circle(x+191.0,y+53.0,4.3,[86,165,229],255);
        ui.text(x+203.0,y+57.7,10.5,false,
                if self.browser_help{"samen://guide"}else{"samen://home"},
                [100,118,140],w-232.0);
        ui.rect(x+14.0,y+78.0,w-28.0,0.6,HAIRLINE,178);
        if self.browser_help{
            ui.round(x+15.0,y+89.0,w-30.0,h-105.0,14.0,[239,246,255],255);
            ui.text(x+35.0,y+125.0,19.5,true,"Welcome to Samen",INK,w-60.0);
            ui.text(x+35.0,y+153.0,11.2,false,"This is a local browser preview.",MUTED,w-65.0);
            ui.text(x+35.0,y+177.0,11.2,false,"Home and Guide tabs respond to clicks.",MUTED,w-65.0);
            ui.text(x+35.0,y+201.0,11.2,false,"Networking and HTML are not available yet.",MUTED,w-65.0);
            ui.round(x+35.0,y+224.0,170.0,27.0,8.0,[215,232,251],255);
            ui.text(x+49.0,y+241.6,10.5,true,"Offline workspace",[47,110,190],147.0);
        }else{
            ui.round(x+15.0,y+89.0,w-30.0,112.0,15.0,[228,241,255],255);
            ui.circle(x+w-70.0,y+143.0,36.0,[167,207,251],86);
            ui.circle(x+w-70.0,y+143.0,26.0,[153,198,247],132);
            ui.glyph_browser(x+w-70.0,y+143.0,1.7);
            ui.text(x+34.0,y+128.0,20.5,true,"Your space. Your OS.",INK,w-133.0);
            ui.text(x+35.0,y+155.0,11.2,false,"A lighter way to explore.",[92,115,144],w-149.0);
            ui.round(x+34.0,y+169.0,139.0,22.0,7.0,[62,126,214],255);
            ui.text(x+47.0,y+184.5,9.7,true,"Samen local home",[255,255,255],125.0);
            ui.text(x+21.0,y+224.0,11.0,true,"Quick access",INK,160.0);
            ui.round(x+20.0,y+237.0,207.0,41.0,10.0,[240,245,252],255);
            ui.glyph_folder(x+42.0,y+257.0,0.65);
            ui.text(x+61.0,y+259.4,10.7,true,"Files",INK,142.0);
            ui.round(x+237.0,y+237.0,215.0,41.0,10.0,[240,245,252],255);
            ui.glyph_terminal(x+260.0,y+257.0,0.67);
            ui.text(x+280.0,y+259.4,10.7,true,"Terminal",INK,160.0);
        }
    }
    fn draw_files(&self,ui:&mut Ui<'_>,r:Rect){
        let (x,y,w,h)=(r.x as f32,r.y as f32,r.w as f32,r.h as f32);
        ui.round(x+14.0,y+38.0,103.0,30.0,8.0,[58,125,213],255);
        ui.text(x+27.0,y+58.0,10.8,true,"+ New sample",[255,255,255],90.0);
        ui.round(x+125.0,y+38.0,85.0,30.0,8.0,[233,241,251],255);
        ui.text(x+142.0,y+57.5,10.8,true,"Delete",[81,107,144],75.0);
        ui.round(x+w-129.0,y+40.0,113.0,26.0,13.0,[227,243,232],255);
        ui.circle(x+w-115.0,y+53.0,3.4,[67,184,114],255);
        ui.text(x+w-105.0,y+57.0,9.7,true,"SamenFS1",[67,133,92],87.0);
        ui.rect(x+13.0,y+77.5,w-26.0,0.7,HAIRLINE,190);
        // Quiet blue sidebar, active root volume, and storage metadata.
        ui.round(x+13.0,y+88.0,100.0,h-103.0,11.0,[236,244,253],255);
        ui.text(x+24.0,y+113.0,9.3,true,"LOCATIONS",MUTED,80.0);
        ui.round(x+19.0,y+124.0,87.0,30.0,7.0,[208,228,252],255);
        ui.glyph_folder(x+35.0,y+139.0,0.51);
        ui.text(x+51.0,y+143.0,10.5,true,"Home",BLUE,48.0);
        ui.text(x+24.0,y+185.0,9.6,false,"Device",MUTED,80.0);
        ui.text(x+24.0,y+203.0,10.2,true,"Primary ATA",INK,79.0);
        ui.text(x+24.0,y+223.0,9.2,false,"Local disk",MUTED,81.0);
        ui.round(x+21.0,y+h-43.0,81.0,4.0,2.0,[213,228,245],255);
        ui.round(x+21.0,y+h-43.0,52.0,4.0,2.0,[99,165,231],255);
        ui.text(x+23.0,y+h-23.0,9.0,false,"SamenFS1",MUTED,80.0);
        // File list with subtle selected highlights and full-resolution text.
        ui.text(x+127.0,y+89.3,9.3,true,"NAME",MUTED,201.0);
        ui.text(x+w-84.0,y+89.3,9.3,true,"SIZE",MUTED,65.0);
        ui.rect(x+126.0,y+93.0,w-140.0,0.5,HAIRLINE,180);
        if self.file_count==0{
            ui.glyph_folder(x+280.0,y+164.0,1.3);
            ui.text(x+184.0,y+199.0,13.8,true,"Nothing here yet",INK,226.0);
            ui.text(x+168.0,y+223.0,10.5,false,"Create a file to get started.",MUTED,245.0);
        }
        let available=((h-146.0)/24.0) as usize;
        for i in 0..self.file_count.min(available).min(7){
            let cy=y+106.0+i as f32*24.0;
            if self.selected_file==Some(i){
                ui.round(x+123.0,cy-9.0,w-136.0,23.0,6.0,[213,232,253],255);
            }else if i%2==1{
                ui.round(x+123.0,cy-9.0,w-136.0,23.0,6.0,[247,250,254],255);
            }
            ui.glyph_folder(x+139.0,cy+1.0,0.43);
            ui.text(x+154.0,cy+4.7,10.4,self.selected_file==Some(i),
                    self.files[i].name(),INK,w-249.0);
            ui.format_bytes(x+w-81.0,cy+4.7,10.0,self.files[i].size,MUTED);
        }
        ui.rect(x+125.0,y+h-49.0,w-139.0,0.6,HAIRLINE,180);
        if let Some(i)=self.selected_file{
            if i<self.file_count{
                ui.text(x+131.0,y+h-31.0,10.8,true,self.files[i].name(),INK,305.0);
                let data=core::str::from_utf8(&self.preview[..self.preview_len.min(70)])
                    .unwrap_or("Binary file preview");
                ui.text(x+131.0,y+h-14.0,9.7,false,data,MUTED,w-160.0);
            }
        }else{
            ui.text(x+131.0,y+h-25.0,10.0,false,"Select a file to preview",MUTED,w-157.0);
        }
    }
    fn draw_terminal(&self,ui:&mut Ui<'_>,r:Rect){
        let (x,y,w,h)=(r.x as f32,r.y as f32,r.w as f32,r.h as f32);
        ui.round(x+14.0,y+39.0,w-28.0,27.0,8.0,[35,55,75],255);
        ui.circle(x+28.0,y+52.5,3.3,[94,229,164],255);
        ui.text(x+40.0,y+57.4,10.8,true,"samen-shell",[205,228,245],166.0);
        ui.text(x+w-139.0,y+57.4,9.6,false,"local  /  x86_64",[157,177,198],125.0);
        ui.rect(x+15.0,y+75.0,w-30.0,0.7,[72,93,116],146);
        ui.set_clip(x+18.0,y+79.0,w-36.0,h-125.0);
        let available=((h-127.0)/15.6) as usize;
        let start=self.terminal.count.saturating_sub(available);
        let mut row=0usize;
        for line in &self.terminal.lines[start..self.terminal.count] {
            let color=match line.style{
                1=>[182,224,254],2=>[113,238,183],3=>[255,136,147],
                4=>[231,241,255],_=>[176,194,215]
            };
            ui.text(x+23.0,y+96.0+row as f32*15.6,10.8,line.style==4,
                    line.value(),color,w-48.0);
            row+=1;
        }
        ui.clear_clip();
        ui.rect(x+16.0,y+h-42.5,w-32.0,0.6,[77,97,119],180);
        ui.text(x+21.0,y+h-21.0,11.2,true,"~ %",[111,234,179],34.0);
        let input=core::str::from_utf8(&self.terminal.input[..self.terminal.input_len])
            .unwrap_or("");
        ui.text(x+57.0,y+h-21.0,11.2,false,input,[234,241,250],w-87.0);
        // Thin insertion caret; no animation is needed, so no flicker.
        let cursor_x=x+57.0+(self.terminal.input_len as f32*6.25).min(w-91.0);
        ui.caret(cursor_x,y+h-16.0,13.0,[105,237,183]);
    }
}
