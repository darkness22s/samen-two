//! Flicker-free native desktop compositor for Samen x86-64.
//!
//! Original 2048x1152 wallpaper is decoded once, then kept at framebuffer
//! resolution. Windows and a real procedural dock are composited into an
//! off-screen surface. Mouse-only movement repairs at most 22x25 pixels.
use alloc::{vec, vec::Vec};
use core::ptr::{copy_nonoverlapping, read_volatile, write_volatile};
use crate::{kprintln, ps2::Mouse, windows::{Manager, Action}};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::{
    bytestream::ZCursor,
    colorspace::ColorSpace,
    options::DecoderOptions,
};

const WALLPAPER: &[u8] = include_bytes!("../assets/source-wallpaper.jpg");
const LOGO_MASK: &[u8] = include_bytes!("../assets/logo-alpha-240x95.bin");
const WALL_W: usize=2048;
const WALL_H: usize=1152;
const LOGO_W: usize=240;
const LOGO_H: usize=95;
const VIRTUAL_W: i32=692;
const VIRTUAL_H: i32=498;

const LIMINE_COMMON: [u64; 2] = [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b];
#[repr(C)]
pub struct FramebufferRequest {
    id: [u64; 4],
    revision: u64,
    response: *const FramebufferResponse,
}
#[repr(C)]
pub struct FramebufferResponse {
    revision: u64,
    count: u64,
    framebuffers: *const *const LimineFramebuffer,
}
#[repr(C)]
pub struct LimineFramebuffer {
    address: *mut u8,
    width: u64,
    height: u64,
    pitch: u64,
    bpp: u16,
    memory_model: u8,
    red_size: u8,
    red_shift: u8,
    green_size: u8,
    green_shift: u8,
    blue_size: u8,
    blue_shift: u8,
    unused: [u8; 7],
    edid_size: u64,
    edid: *const u8,
}
#[used]
#[unsafe(link_section = ".limine_reqs")]
static mut FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest {
    id: [LIMINE_COMMON[0], LIMINE_COMMON[1], 0x9d5827dcd881dd75, 0xa3148604f6fab11b],
    revision: 0,
    response: core::ptr::null(),
};

pub struct Framebuffer {
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    red_shift: u8,
    green_shift: u8,
    blue_shift: u8,
    // Both buffers are normal RAM; the hardware framebuffer is write-only here.
    wallpaper: Vec<u32>,
    desktop_cache: Vec<u32>,
    cached_variant: Option<Option<App>>,
    scene: Vec<u32>,
}
pub unsafe fn acquire() -> Option<Framebuffer> {
    let response=unsafe {read_volatile(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response))};
    if response.is_null(){return None;}
    let r=unsafe{&*response};
    if r.count==0 || r.framebuffers.is_null(){return None;}
    let fb=unsafe{&**r.framebuffers};
    let (w,h,p)=(fb.width as usize,fb.height as usize,fb.pitch as usize);
    if fb.address.is_null() || fb.bpp!=32 || fb.memory_model!=1
       || fb.red_size!=8 || fb.green_size!=8 || fb.blue_size!=8
       || w<320 || h<240 || w>4096 || h>2160 || p<w*4
       || p.checked_mul(h).is_none(){return None;}
    let len=w.checked_mul(h)?;
    Some(Framebuffer{
        address:fb.address,width:w,height:h,pitch:p,
        red_shift:fb.red_shift,green_shift:fb.green_shift,blue_shift:fb.blue_shift,
        wallpaper:vec![0;len],desktop_cache:vec![0;len],cached_variant:None,scene:vec![0;len],
    })
}
impl Framebuffer {
    pub fn dimensions(&self)->(usize,usize){(self.width,self.height)}
    fn pack(&self,rgb:[u8;3])->u32 {
        (u32::from(rgb[0])<<self.red_shift)
        | (u32::from(rgb[1])<<self.green_shift)
        | (u32::from(rgb[2])<<self.blue_shift)
    }
    pub(crate) fn desktop_scale(&self)->f32 { self.scale() }
    fn scale(&self)->f32 {
        (self.width as f32/VIRTUAL_W as f32).min(self.height as f32/VIRTUAL_H as f32)
    }
    fn origin(&self)->(i32,i32) {
        let s=self.scale();
        (((self.width as f32-(VIRTUAL_W as f32)*s)*0.5) as i32,
         ((self.height as f32-(VIRTUAL_H as f32)*s)*0.5) as i32)
    }
    pub fn from_screen(&self,x:i32,y:i32)->(i32,i32){
        let (ox,oy)=self.origin();
        let s=self.scale();
        (((x-ox) as f32/s) as i32,((y-oy) as f32/s) as i32)
    }
    pub fn to_screen(&self,x:i32,y:i32)->(i32,i32){
        let (ox,oy)=self.origin();
        let s=self.scale();
        (ox+(x as f32*s) as i32,oy+(y as f32*s) as i32)
    }
    pub fn install_wallpaper(&mut self,image:&[u8])->bool {
        if image.len()!=WALL_W*WALL_H*3 {return false;}
        // "cover" sampling, preserving aspect ratio; no 692x498 screenshot
        // is ever used as a background. The original file is 2048x1152.
        let (cw,ch)=if self.width*WALL_H>=self.height*WALL_W {
            (WALL_W,(WALL_W*self.height/self.width).min(WALL_H))
        } else {
            ((WALL_H*self.width/self.height).min(WALL_W),WALL_H)
        };
        let left=(WALL_W-cw)/2;
        let top=(WALL_H-ch)/2;
        let rs=self.red_shift;let gs=self.green_shift;let bs=self.blue_shift;
        for y in 0..self.height {
            let sy=(top+y*ch/self.height).min(WALL_H-1);
            for x in 0..self.width {
                let sx=(left+x*cw/self.width).min(WALL_W-1);
                let si=(sy*WALL_W+sx)*3;
                self.wallpaper[y*self.width+x]=
                    (u32::from(image[si])<<rs)
                  | (u32::from(image[si+1])<<gs)
                  | (u32::from(image[si+2])<<bs);
            }
        }
        true
    }
    pub fn begin_scene(&mut self){self.scene.copy_from_slice(&self.desktop_cache);}
    fn needs_cache(&self, selected:Option<App>)->bool{
        self.cached_variant!=Some(selected)
    }
    fn update_cache(&mut self,selected:Option<App>){
        self.desktop_cache.copy_from_slice(&self.scene);
        self.cached_variant=Some(selected);
    }
    pub(crate) fn blend_native(&mut self,x:i32,y:i32,c:[u8;3],alpha:u8){
        if alpha==0||x<0||y<0||x>=self.width as i32||y>=self.height as i32{return;}
        let idx=y as usize*self.width+x as usize;
        if alpha==255{self.scene[idx]=self.pack(c);return;}
        let old=self.scene[idx];let a=u32::from(alpha);
        let r=(u32::from(c[0])*a+((old>>self.red_shift)&255)*(255-a)+127)/255;
        let g=(u32::from(c[1])*a+((old>>self.green_shift)&255)*(255-a)+127)/255;
        let b=(u32::from(c[2])*a+((old>>self.blue_shift)&255)*(255-a)+127)/255;
        self.scene[idx]=(r<<self.red_shift)|(g<<self.green_shift)|(b<<self.blue_shift);
    }
    pub fn present(&mut self){
        // The complete image is composed off-screen; there is never a visible
        // partial redraw with a blank wallpaper or missing windows.
        for y in 0..self.height {
            // SAFETY: validated dimensions and mapped Limine framebuffer.
            unsafe {
                let dst=self.address.add(y*self.pitch).cast::<u32>();
                let src=self.scene.as_ptr().add(y*self.width);
                copy_nonoverlapping(src,dst,self.width);
            }
        }
    }
    fn cursor_pixel(&mut self,x:usize,y:usize,rgb:[u8;3]){
        if x>=self.width||y>=self.height{return;}
        let val=self.pack(rgb);
        unsafe {write_volatile(self.address.add(y*self.pitch+x*4).cast::<u32>(),val);}
    }
    pub fn restore_cursor(&mut self,x:i32,y:i32){
        if x<0 || y<0 {return;}
        let x=x as usize;let y=y as usize;
        if x>=self.width||y>=self.height{return;}
        let xmax=(x+22).min(self.width);
        let ymax=(y+26).min(self.height);
        for row in y..ymax {
            // Restore the actual composited pixels behind the pointer, not
            // the original wallpaper; prevents window/dock smearing.
            unsafe {
                let src=self.scene.as_ptr().add(row*self.width+x);
                let dst=self.address.add(row*self.pitch+x*4).cast::<u32>();
                copy_nonoverlapping(src,dst,xmax-x);
            }
        }
    }
    pub fn draw_cursor(&mut self,x:i32,y:i32){
        if x<0||y<0{return;}
        const ARROW:[&str;19]=[
            "X...............","XX..............","XOX.............",
            "XOOX............","XOOOX...........","XOOOOX..........",
            "XOOOOOX.........","XOOOOOOX........","XOOOOOOOX.......",
            "XOOOOOOOOX......","XOOOOOOOOOX.....","XOOOOXXXXX......",
            "XOOOX...........","XOOX............","XXX.............",
            "XX..............","X...............","................",
            "................",
        ];
        for (dy,line) in ARROW.iter().enumerate(){
            for (dx,c) in line.bytes().enumerate(){
                if c!=b'.'{
                    self.cursor_pixel(x as usize+dx,y as usize+dy,
                        if c==b'O'{[255,255,255]}else{[17,30,51]});
                }
            }
        }
    }
    pub(crate) fn logical_blend(&mut self,x:i32,y:i32,c:[u8;3],alpha:u8){
        if x<0||y<0||x>=VIRTUAL_W||y>=VIRTUAL_H||alpha==0{return;}
        let s=self.scale();
        let (ox,oy)=self.origin();
        let x0=ox+(x as f32*s) as i32;
        let x1=ox+(((x+1) as f32)*s) as i32;
        let y0=oy+(y as f32*s) as i32;
        let y1=oy+(((y+1) as f32)*s) as i32;
        for sy in y0.max(0) as usize..y1.max(0).min(self.height as i32) as usize{
            for sx in x0.max(0) as usize..x1.max(0).min(self.width as i32) as usize {
                let idx=sy*self.width+sx;
                let old=self.scene[idx];
                self.scene[idx]=if alpha==255 {self.pack(c)}else{
                    let a=u32::from(alpha);
                    let red=(u32::from(c[0])*a+((old>>self.red_shift)&255)*(255-a)+127)/255;
                    let green=(u32::from(c[1])*a+((old>>self.green_shift)&255)*(255-a)+127)/255;
                    let blue=(u32::from(c[2])*a+((old>>self.blue_shift)&255)*(255-a)+127)/255;
                    (red<<self.red_shift)|(green<<self.green_shift)|(blue<<self.blue_shift)
                };
            }
        }
    }
}

#[derive(Clone,Copy,PartialEq,Eq,Debug)]
pub enum App {Browser,Files,Settings}
impl App {
    pub fn label(self)->&'static str {
        match self{Self::Browser=>"Browser",Self::Files=>"Files",Self::Settings=>"Settings"}
    }
}
pub struct Desktop {
    pub x:i32,
    pub y:i32,
    pub selected:Option<App>,
    pub windows:Manager,
    pub scene_redraws:u64,
    pub cursor_updates:u64,
}
impl Desktop {
    pub fn initialize(fb:&mut Framebuffer)->Option<Self>{
        // A single JPEG decode at boot instead of scaling and recomposing a
        // tiny, baked-in desktop image on every mouse movement.
        let opts=DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
        let mut decoder=JpegDecoder::new_with_options(ZCursor::new(WALLPAPER),opts);
        let pixels=decoder.decode().ok()?;
        if !fb.install_wallpaper(&pixels){return None;}
        let mut desktop=Self{x:(fb.width/2) as i32,y:(fb.height/2) as i32,
            selected:None,windows:Manager::new(),scene_redraws:0,cursor_updates:0};
        desktop.paint_all(fb);
        Some(desktop)
    }
    fn paint_logo(&self,fb:&mut Framebuffer){
        // Interpolated alpha at *native screen pixels*: no 2x2 bitmap blocks.
        let (ox,oy)=fb.to_screen(237,96);
        let scale=fb.scale();
        let ow=(LOGO_W as f32*scale+0.999) as i32;
        let oh=(LOGO_H as f32*scale+0.999) as i32;
        for py in 0..oh{
            let fy=((py as f32+0.5)/scale-0.5).clamp(0.0,(LOGO_H-1) as f32);
            let sy=fy as usize;
            let sy1=(sy+1).min(LOGO_H-1);
            let ty=fy-sy as f32;
            for px in 0..ow{
                let fx=((px as f32+0.5)/scale-0.5).clamp(0.0,(LOGO_W-1) as f32);
                let sx=fx as usize;
                let sx1=(sx+1).min(LOGO_W-1);
                let tx=fx-sx as f32;
                let a00=LOGO_MASK[sy*LOGO_W+sx] as f32;
                let a10=LOGO_MASK[sy*LOGO_W+sx1] as f32;
                let a01=LOGO_MASK[sy1*LOGO_W+sx] as f32;
                let a11=LOGO_MASK[sy1*LOGO_W+sx1] as f32;
                let a=(a00*(1.0-tx)*(1.0-ty)+a10*tx*(1.0-ty)
                      +a01*(1.0-tx)*ty+a11*tx*ty+0.5) as u8;
                fb.blend_native(ox+px,oy+py,[255,255,255],a);
            }
        }
    }
    pub fn paint_all(&mut self,fb:&mut Framebuffer){
        self.scene_redraws+=1;
        // Expensive HD MSAA graphics are rasterized only when the dock's
        // selected state changes, never for window motion or file operations.
        if fb.needs_cache(self.selected){
            fb.scene.copy_from_slice(&fb.wallpaper);
            self.paint_logo(fb);
            self.paint_dock(fb);
            fb.update_cache(self.selected);
        }
        fb.begin_scene();
        self.windows.draw(fb);
        fb.present();
        fb.draw_cursor(self.x,self.y);
    }
    fn paint_dock(&self,fb:&mut Framebuffer){
        crate::dock::draw(fb,self.selected);
    }
    fn icon_box(&self,app:App)->(i32,i32,i32,i32){
        let (a,b)=match app{App::Browser=>(287,334),App::Files=>(334,382),App::Settings=>(382,432)};
        (a,422,b,471)
    }
    pub fn hit_test(&self,fb:&Framebuffer,x:usize,y:usize)->Option<App>{
        let (lx,ly)=fb.from_screen(x as i32,y as i32);
        for app in [App::Browser,App::Files,App::Settings]{
            let (x0,y0,x1,y1)=self.icon_box(app);
            if lx>=x0 && ly>=y0 && lx<x1 && ly<y1{return Some(app);}
        }
        None
    }
    pub fn choose(&mut self,fb:&mut Framebuffer,app:App){
        self.selected=Some(app);
        self.windows.open(app);
        self.paint_all(fb);
        kprintln!("GUI: selected {}",app.label());
        kprintln!("GUI: opened {} window",app.label());
    }
    pub fn on_mouse(&mut self,fb:&mut Framebuffer,dx:i32,dy:i32,
        left_pressed:bool,left_down:bool,fs:Option<&samen_kernel::fs::Fs>)->Action {
        let old_x=self.x;let old_y=self.y;
        let speed=if self.windows.fast_pointer{2}else{1};
        self.x=(self.x+dx*speed).clamp(0,fb.width.saturating_sub(1) as i32);
        self.y=(self.y+dy*speed).clamp(0,fb.height.saturating_sub(1) as i32);
        let (lx,ly)=fb.from_screen(self.x,self.y);
        let drag_before=self.windows.is_dragging();
        self.windows.on_move(lx,ly,left_down);
        let mut action=Action::None;
        let mut repaint=false;
        if left_pressed{
            if let Some(app)=self.hit_test(fb,self.x as usize,self.y as usize){
                self.selected=Some(app);
                if app==App::Files{self.windows.sync_files(fs);}
                self.windows.open(app);
                kprintln!("GUI: selected {}",app.label());
                kprintln!("GUI: opened {} window",app.label());
            }else {
                action=self.windows.click(lx,ly);
            }
            repaint=true;
        }
        if !left_down {self.windows.release();}
        // Entire scene is recomposed only when window state actually changes.
        // Pure pointer motion touches fewer than 600 pixels, not 2 million.
        if action!=Action::None{
            // The file action handler will redraw once after the storage
            // operation completes; skip the redundant pre-action redraw.
        }else if repaint || (drag_before && (dx!=0 || dy!=0)){
            self.paint_all(fb);
        } else {
            self.cursor_updates+=1;
            fb.restore_cursor(old_x,old_y);
            fb.draw_cursor(self.x,self.y);
        }
        action
    }
    pub fn refresh_files(&mut self,fb:&mut Framebuffer,fs:Option<&samen_kernel::fs::Fs>){
        self.windows.sync_files(fs);
        if self.windows.is_open(App::Files){self.paint_all(fb);}
    }
    pub fn selected_name(&self)->&'static str {
        self.selected.map(|a|a.label()).unwrap_or("none")
    }
}
pub fn mouse_init()->Option<Mouse>{Mouse::init()}
