//! Samen GUI Phase 1: direct Limine framebuffer desktop with the user's
//! exported Figma artwork, PS/2 cursor, dock hit testing and launch events.
//! Browser / Files / Settings window interiors are deliberately Phase 2.
use core::ptr::{read_volatile, write_volatile};
use crate::{kprintln, ps2::Mouse, windows::{Manager, Action}};


const DESKTOP_QOI: &[u8] = include_bytes!("../assets/samen-desktop.qoi");
const ART_WIDTH: usize = 692;
const ART_HEIGHT: usize = 498;
const ART_PIXELS: usize = ART_WIDTH * ART_HEIGHT * 3;
// Exact screenshot pixels, decoded once. This does not require a heap.
static mut BACKGROUND: [u8; ART_PIXELS] = [0; ART_PIXELS];

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
}

pub unsafe fn acquire() -> Option<Framebuffer> {
    // SAFETY: Limine writes the response pointer before entering _start.
    let response = unsafe { read_volatile(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response)) };
    if response.is_null() { return None; }
    let r = unsafe { &*response };
    if r.count == 0 || r.framebuffers.is_null() { return None; }
    let fb = unsafe { &**r.framebuffers };
    let (w,h,p) = (fb.width as usize, fb.height as usize, fb.pitch as usize);
    if fb.address.is_null() || fb.bpp != 32 || fb.memory_model != 1
        || fb.red_size != 8 || fb.green_size != 8 || fb.blue_size != 8
        || w < 320 || h < 240 || w > 4096 || h > 2160 || p < w * 4
        || p.checked_mul(h).is_none() {
        return None;
    }
    Some(Framebuffer {
        address: fb.address,
        width: w,
        height: h,
        pitch: p,
        red_shift: fb.red_shift,
        green_shift: fb.green_shift,
        blue_shift: fb.blue_shift,
    })
}

impl Framebuffer {
    fn pixel(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        if x >= self.width || y >= self.height { return; }
        let color = (u32::from(rgb[0]) << self.red_shift)
                  | (u32::from(rgb[1]) << self.green_shift)
                  | (u32::from(rgb[2]) << self.blue_shift);
        // SAFETY: validated 32-bpp framebuffer, pitch and dimensions from Limine.
        unsafe {
            let addr = self.address.add(y * self.pitch + x * 4).cast::<u32>();
            write_volatile(addr, color);
        }
    }

    pub fn dimensions(&self) -> (usize, usize) { (self.width, self.height) }

    // Logical 692x498 desktop-space coordinates, with straight-alpha compositing
    // against existing framebuffer contents. Bounded to Limine's mapped surface.
    pub(crate) fn logical_blend(&mut self,x:i32,y:i32,c:[u8;3],alpha:u8) {
        if x<0||y<0||x>=692||y>=498{return;}
        let x0=x as usize*self.width/692;
        let x1=((x+1) as usize*self.width/692).max(x0+1).min(self.width);
        let y0=y as usize*self.height/498;
        let y1=((y+1) as usize*self.height/498).max(y0+1).min(self.height);
        for py in y0..y1 {
            for px in x0..x1 {
                let offset=py*self.pitch+px*4;
                // SAFETY: all offsets are in framebuffer bounds established at boot.
                unsafe {
                    let pixel=self.address.add(offset).cast::<u32>();
                    let previous=read_volatile(pixel);
                    let shifts=[self.red_shift,self.green_shift,self.blue_shift];
                    let mut output=0u32;
                    for i in 0..3 {
                        let old=((previous>>shifts[i])&255) as u32;
                        let new=(u32::from(c[i])*u32::from(alpha)+old*(255-u32::from(alpha))+127)/255;
                        output|=new<<shifts[i];
                    }
                    write_volatile(pixel,output);
                }
            }
        }
    }
}

/// Files are real mouse targets; desktop artwork itself is never redrawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum App { Browser, Files, Settings }
impl App {
    pub fn label(self) -> &'static str {
        match self { Self::Browser => "Browser", Self::Files => "Files", Self::Settings => "Settings" }
    }
}
pub struct Desktop {
    pixels: &'static [u8],
    pub x: i32,
    pub y: i32,
    pub selected: Option<App>,
    pub windows: Manager,
}
impl Desktop {
    pub fn initialize(fb: &mut Framebuffer) -> Option<Self> {
        // SAFETY: exclusive one-time initialization before event loop; static lives forever.
        let pixels: &'static mut [u8; ART_PIXELS] = unsafe { &mut *core::ptr::addr_of_mut!(BACKGROUND) };
        if !samen_kernel::qoi::decode_rgb(DESKTOP_QOI, pixels) { return None; }
        let mut desktop = Self {
            pixels,
            x: (fb.width / 2) as i32,
            y: (fb.height / 2) as i32,
            selected: None,
            windows: Manager::new(),
        };
        desktop.paint_all(fb);
        Some(desktop)
    }

    fn art_pixel(&self, x: usize, y: usize, fb: &Framebuffer) -> [u8; 3] {
        // Scale the exact composed Figma frame, retaining original layer positions.
        let sx = x * ART_WIDTH / fb.width;
        let sy = y * ART_HEIGHT / fb.height;
        let index = (sy * ART_WIDTH + sx) * 3;
        [self.pixels[index], self.pixels[index + 1], self.pixels[index + 2]]
    }

    fn paint_region(&self, fb: &mut Framebuffer, x0: usize, y0: usize, x1: usize, y1: usize) {
        for y in y0..y1.min(fb.height) {
            for x in x0..x1.min(fb.width) {
                let color = self.art_pixel(x, y, fb);
                fb.pixel(x, y, color);
            }
        }
    }
    pub fn paint_all(&mut self, fb: &mut Framebuffer) {
        self.paint_region(fb, 0, 0, fb.width, fb.height);
        self.windows.draw(fb);
        self.paint_selection(fb);
        self.paint_cursor(fb);
    }
    fn cursor_bounds(&self) -> (usize, usize, usize, usize) {
        let x = self.x.max(0) as usize;
        let y = self.y.max(0) as usize;
        (x, y, x + 18, y + 22)
    }
    fn paint_cursor(&self, fb: &mut Framebuffer) {
        // Small, recognizable software pointer with one-pixel dark edging.
        const ARROW: [&str; 18] = [
            "X...........", "XX..........", "XOX.........",
            "XOOX........", "XOOOX.......", "XOOOOX......",
            "XOOOOOX.....", "XOOOOOOX....", "XOOOOOOOX...",
            "XOOOOOOOOX..", "XOOOOOOOOOX.", "XOOOOXXXXXX.",
            "XOOOX.......", "XOOX........", "XXX.........",
            "XX..........", "X...........", "............",
        ];
        let (cx, cy) = (self.x.max(0) as usize, self.y.max(0) as usize);
        for (dy, line) in ARROW.iter().enumerate() {
            for (dx, c) in line.bytes().enumerate() {
                if c == b'.' { continue; }
                fb.pixel(cx + dx, cy + dy, if c == b'O' { [255,255,255] } else { [22,30,48] });
            }
        }
    }
    fn icon_box(&self, fb: &Framebuffer, app: App) -> (usize, usize, usize, usize) {
        let (x0, x1) = match app { App::Browser => (289, 334), App::Files => (334, 381), App::Settings => (383, 432) };
        (x0 * fb.width / ART_WIDTH, 423 * fb.height / ART_HEIGHT,
         x1 * fb.width / ART_WIDTH, 470 * fb.height / ART_HEIGHT)
    }
    pub fn hit_test(&self, fb: &Framebuffer, x: usize, y: usize) -> Option<App> {
        for app in [App::Browser, App::Files, App::Settings] {
            let (x0,y0,x1,y1) = self.icon_box(fb, app);
            if x >= x0 && x < x1 && y >= y0 && y < y1 { return Some(app); }
        }
        None
    }
    fn paint_selection(&self, fb: &mut Framebuffer) {
        let Some(app) = self.selected else { return };
        let (x0,y0,x1,y1) = self.icon_box(fb, app);
        let color = [246,246,255];
        // Visual feedback only after a click; the initial desktop is unchanged.
        for x in x0..x1 {
            fb.pixel(x,y0,color);
            fb.pixel(x,y1-1,color);
        }
        for y in y0..y1 {
            fb.pixel(x0,y,color);
            fb.pixel(x1-1,y,color);
        }
    }
    pub fn choose(&mut self, fb: &mut Framebuffer, app: App) {
        self.selected = Some(app);
        self.windows.open(app);
        self.paint_all(fb);
        kprintln!("GUI: selected {}", app.label());
        kprintln!("GUI: opened {} window", app.label());
    }
    pub fn on_mouse(&mut self,fb:&mut Framebuffer,dx:i32,dy:i32,
        left_pressed:bool,left_down:bool)->Action {
        let speed=if self.windows.fast_pointer {2}else{1};
        self.x=(self.x+dx*speed).clamp(0,fb.width.saturating_sub(1) as i32);
        self.y=(self.y+dy*speed).clamp(0,fb.height.saturating_sub(1) as i32);
        let lx=(self.x as i64*692/fb.width as i64) as i32;
        let ly=(self.y as i64*498/fb.height as i64) as i32;
        self.windows.on_move(lx,ly,left_down);
        let mut action=Action::None;
        if left_pressed {
            if let Some(app)=self.hit_test(fb,self.x as usize,self.y as usize) {
                self.choose(fb,app);
            } else {
                action=self.windows.click(lx,ly);
            }
        }
        if !left_down {self.windows.release();}
        self.paint_all(fb);
        action
    }
    pub fn refresh_files(&mut self,fb:&mut Framebuffer,fs:Option<&samen_kernel::fs::Fs>){
        self.windows.sync_files(fs);
        if self.windows.is_open(App::Files){self.paint_all(fb);}
    }
    pub fn selected_name(&self) -> &'static str {
        self.selected.map(|a| a.label()).unwrap_or("none")
    }
}

/// Called on graphical boot. Keep polling mouse without taking over serial shell.
pub fn mouse_init() -> Option<Mouse> { Mouse::init() }
