//! Native-resolution window UI primitives and antialiased Inter-derived glyphs.
//! Rectangles, line-work and text are all rasterized in actual screen pixels.
//! The 692x498 layout coordinates describe placement only, never pixel blocks.
use crate::gui::Framebuffer;
#[path = "font_data.rs"]
mod font_data;
use font_data::{Glyph,GLYPHS};

const ALPHA:&[u8]=include_bytes!("../assets/ui-inter-glyphs.bin");
const GLYPH_BASE:f32=48.0;
type Color=[u8;3];
const SAMPLES:[f32;4]=[0.125,0.375,0.625,0.875];

pub struct Ui<'a>{
    fb:&'a mut Framebuffer,
    scale:f32,
    ox:f32,
    oy:f32,
    clip:Option<(i32,i32,i32,i32)>,
}
impl<'a> Ui<'a>{
    pub fn new(fb:&'a mut Framebuffer)->Self{
        let scale=fb.desktop_scale();
        let (ox,oy)=fb.to_screen(0,0);
        Self{fb,scale,ox:ox as f32,oy:oy as f32,clip:None}
    }
    pub fn set_clip(&mut self,x:f32,y:f32,w:f32,h:f32){
        let (l,t)=self.map(x,y);
        self.clip=Some((l as i32,t as i32,(l+w*self.scale) as i32,(t+h*self.scale) as i32));
    }
    pub fn clear_clip(&mut self){self.clip=None;}
    fn map(&self,x:f32,y:f32)->(f32,f32){
        (self.ox+x*self.scale,self.oy+y*self.scale)
    }
    fn visible(&self,x:i32,y:i32)->bool{
        if let Some((x0,y0,x1,y1))=self.clip{
            x>=x0&&x<x1&&y>=y0&&y<y1
        }else{true}
    }
    fn pixel(&mut self,x:i32,y:i32,color:Color,alpha:u8){
        if alpha>0&&self.visible(x,y){self.fb.blend_native(x,y,color,alpha);}
    }
    fn area<F>(&mut self,x:f32,y:f32,w:f32,h:f32,color:Color,opacity:u8,shape:F)
    where F:Fn(f32,f32)->bool{
        if w<=0.0||h<=0.0||opacity==0{return;}
        let min_x=(x as i32-2).max(0);
        let min_y=(y as i32-2).max(0);
        let (sw,sh)=self.fb.dimensions();
        let max_x=((x+w+2.0) as i32).min(sw as i32);
        let max_y=((y+h+2.0) as i32).min(sh as i32);
        for py in min_y..max_y{
            for px in min_x..max_x{
                if !self.visible(px,py){continue;}
                let xf=px as f32;
                let yf=py as f32;
                // Do a fast opaque/interior path when every corner is inside.
                if shape(xf+0.01,yf+0.01)&&shape(xf+0.99,yf+0.01)
                    &&shape(xf+0.01,yf+0.99)&&shape(xf+0.99,yf+0.99){
                    self.fb.blend_native(px,py,color,opacity);
                    continue;
                }
                let mut n=0u32;
                for sy in SAMPLES{for sx in SAMPLES{
                    if shape(xf+sx,yf+sy){n+=1;}
                }}
                if n>0{
                    self.fb.blend_native(px,py,color,((n*u32::from(opacity)+8)/16) as u8);
                }
            }
        }
    }
    pub fn round(&mut self,x:f32,y:f32,w:f32,h:f32,r:f32,color:Color,a:u8){
        let (l,t)=self.map(x,y);
        let (w,h,r)=(w*self.scale,h*self.scale,r*self.scale);
        self.area(l,t,w,h,color,a,|px,py|{
            if px<l||py<t||px>=l+w||py>=t+h{return false;}
            let radius=r.min(w*0.5).min(h*0.5);
            let dx=if px<l+radius{l+radius-px}else if px>l+w-radius{px-(l+w-radius)}else{0.0};
            let dy=if py<t+radius{t+radius-py}else if py>t+h-radius{py-(t+h-radius)}else{0.0};
            dx*dx+dy*dy<=radius*radius
        });
    }
    pub fn rect(&mut self,x:f32,y:f32,w:f32,h:f32,c:Color,a:u8){
        let (l,t)=self.map(x,y);
        let (w,h)=(w*self.scale,h*self.scale);
        self.area(l,t,w,h,c,a,|px,py|px>=l&&py>=t&&px<l+w&&py<t+h);
    }
    pub fn circle(&mut self,cx:f32,cy:f32,r:f32,c:Color,a:u8){
        let (x,y)=self.map(cx,cy);
        let r=r*self.scale;
        self.area(x-r,y-r,r*2.0,r*2.0,c,a,|px,py|{
            let dx=px-x;
            let dy=py-y;
            dx*dx+dy*dy<=r*r
        });
    }
    pub fn ring(&mut self,cx:f32,cy:f32,r:f32,thickness:f32,c:Color,a:u8){
        let (x,y)=self.map(cx,cy);
        let (r,inner)=(r*self.scale,(r-thickness).max(0.0)*self.scale);
        self.area(x-r,y-r,r*2.0,r*2.0,c,a,|px,py|{
            let dx=px-x;let dy=py-y;let d=dx*dx+dy*dy;
            d<=r*r&&d>=inner*inner
        });
    }
    pub fn line(&mut self,x0:f32,y0:f32,x1:f32,y1:f32,thick:f32,c:Color,a:u8){
        let (x0,y0)=self.map(x0,y0);
        let (x1,y1)=self.map(x1,y1);
        let rad=thick*self.scale*0.5;
        let left=x0.min(x1)-rad-1.0;
        let top=y0.min(y1)-rad-1.0;
        let w=(x1.max(x0)-left)+rad+2.0;
        let h=(y1.max(y0)-top)+rad+2.0;
        self.area(left,top,w,h,c,a,|x,y|{
            let vx=x1-x0;let vy=y1-y0;
            let d=vx*vx+vy*vy;
            let k=if d==0.0{0.0}else{((x-x0)*vx+(y-y0)*vy)/d};
            let k=k.clamp(0.0,1.0);
            let dx=x-(x0+k*vx);let dy=y-(y0+k*vy);
            dx*dx+dy*dy<=rad*rad
        });
    }
    pub fn text(&mut self,x:f32,baseline:f32,size:f32,bold:bool,string:&str,c:Color,max_width:f32){
        let mut pen=x;
        let right=x+max_width;
        for ch in string.bytes().take(150){
            if pen>=right{break;}
            if !(32..=126).contains(&ch){continue;}
            let g=GLYPHS[usize::from(bold)][(ch-32) as usize];
            let advance=g.advance as f32/64.0*size/GLYPH_BASE;
            if pen+advance>right+0.01{break;}
            self.glyph(pen,baseline,size,g,c);
            pen+=advance;
        }
    }
    fn glyph(&mut self,x:f32,baseline:f32,size:f32,g:Glyph,c:Color){
        if g.w==0||g.h==0{return;}
        let (l,t)=self.map(x+f32::from(g.x)*size/GLYPH_BASE,
                             baseline+f32::from(g.y)*size/GLYPH_BASE);
        let ratio=size*self.scale/GLYPH_BASE;
        if ratio<=0.0{return;}
        let max_x=(l+g.w as f32*ratio+0.999) as i32;
        let max_y=(t+g.h as f32*ratio+0.999) as i32;
        let min_x=(l as i32-1).max(0);
        let min_y=(t as i32-1).max(0);
        let (screen_w,screen_h)=self.fb.dimensions();
        for py in min_y..max_y.min(screen_h as i32){
            for px in min_x..max_x.min(screen_w as i32){
                if !self.visible(px,py){continue;}
                let fx=(px as f32+0.5-l)/ratio-0.5;
                let fy=(py as f32+0.5-t)/ratio-0.5;
                if fx< -0.5||fy< -0.5||fx>g.w as f32-0.5||fy>g.h as f32-0.5{continue;}
                let ix=(fx as i32).clamp(0,g.w as i32-1) as usize;
                let iy=(fy as i32).clamp(0,g.h as i32-1) as usize;
                let nx=(ix+1).min(g.w as usize-1);
                let ny=(iy+1).min(g.h as usize-1);
                let tx=(fx-ix as f32).clamp(0.0,1.0);
                let ty=(fy-iy as f32).clamp(0.0,1.0);
                let off=g.offset as usize;
                let pitch=g.w as usize;
                let src=|xx:usize,yy:usize| ALPHA[off+yy*pitch+xx] as f32;
                let a=src(ix,iy)*(1.0-tx)*(1.0-ty)
                    +src(nx,iy)*tx*(1.0-ty)
                    +src(ix,ny)*(1.0-tx)*ty
                    +src(nx,ny)*tx*ty;
                if a>=1.0{self.fb.blend_native(px,py,c,(a+0.5) as u8);}
            }
        }
    }
    pub fn format_bytes(&mut self,x:f32,y:f32,size:f32,n:u32,c:Color){
        let mut bytes=[0;11];let mut i=bytes.len();let mut val=n;
        loop{i-=1;bytes[i]=b'0'+(val%10) as u8;val/=10;if val==0{break;}}
        if let Ok(v)=core::str::from_utf8(&bytes[i..]){
            self.text(x,y,size,false,v,c,95.0);
        }
    }
    pub fn glyph_folder(&mut self,x:f32,y:f32,s:f32){
        self.round(x-14.0*s,y-10.0*s,16.0*s,8.0*s,2.7*s,[71,143,210],255);
        self.round(x-15.0*s,y-6.0*s,30.0*s,22.0*s,4.0*s,[60,153,227],255);
        self.round(x-13.0*s,y-3.0*s,26.0*s,17.0*s,3.2*s,[137,205,254],255);
        self.line(x-11.0*s,y+1.0*s,x+11.0*s,y+1.0*s,0.9*s,[224,245,255],210);
    }
    pub fn glyph_terminal(&mut self,x:f32,y:f32,s:f32){
        self.round(x-13.0*s,y-11.0*s,26.0*s,22.0*s,5.0*s,[24,41,61],255);
        self.line(x-8.0*s,y-5.0*s,x-3.0*s,y,1.9*s,[130,239,196],255);
        self.line(x-3.0*s,y,x-8.0*s,y+5.0*s,1.9*s,[130,239,196],255);
        self.line(x-1.0*s,y+5.5*s,x+8.0*s,y+5.5*s,1.6*s,[219,236,249],255);
    }
    pub fn glyph_browser(&mut self,x:f32,y:f32,s:f32){
        self.circle(x,y,13.0*s,[255,255,255],255);
        self.circle(x,y,11.7*s,[67,166,102],255);
        self.circle(x,y,7.0*s,[245,249,255],255);
        self.circle(x,y,5.7*s,[54,140,230],255);
    }
    pub fn caret(&mut self,x:f32,y:f32,h:f32,c:Color){
        self.round(x,y-h,1.8,h,0.6,c,220);
    }
}
