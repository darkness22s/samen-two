//! Device-resolution, antialiased UI primitives and an embedded rasterized
//! proportional font atlas (derived from freely redistributable DejaVu Sans).
//! Coordinates stay in Samen's 692x498 layout space; rasterization DOES NOT.
use crate::{gui::Framebuffer,windows::Rect};

const ATLAS:&[u8]=include_bytes!("../assets/samen-ui-alpha-font.bin");
const GW:usize=24;
const GH:usize=32;
const CELL:usize=1+GW*GH;
const HEADER:usize=8;
pub type Color=[u8;3];

fn device_bounds(fb:&Framebuffer,r:Rect)->(i32,i32,i32,i32){
    let (x,y)=fb.to_screen(r.x,r.y);
    let (ex,ey)=fb.to_screen(r.x+r.w,r.y+r.h);
    (x,y,ex,ey)
}
#[inline]
fn rounded_hit(x:f32,y:f32,l:f32,t:f32,w:f32,h:f32,r:f32)->bool {
    if x<l||x>=l+w||y<t||y>=t+h{return false}
    let r=r.min(w/2.0).min(h/2.0);
    let dx=if x<l+r{l+r-x}else if x>l+w-r{x-l-w+r}else{0.0};
    let dy=if y<t+r{t+r-y}else if y>t+h-r{y-t-h+r}else{0.0};
    dx*dx+dy*dy<=r*r
}

pub fn round(fb:&mut Framebuffer,r:Rect,radius:f32,color:Color,alpha:u8){
    if r.w<=0||r.h<=0||alpha==0{return;}
    let (l,t,e,b)=device_bounds(fb,r);
    let (sw,sh)=fb.dimensions();
    let (w,h)=((e-l) as f32,(b-t) as f32);
    let rad=radius*fb.desktop_scale();
    for py in t.max(0)..b.min(sh as i32){
        for px in l.max(0)..e.min(sw as i32){
            // Uniform body pixel, with supersampling only near the curved edge.
            let x=px as f32;let y=py as f32;
            let full=rounded_hit(x+0.02,y+0.02,l as f32,t as f32,w,h,rad)
                &&rounded_hit(x+0.98,y+0.02,l as f32,t as f32,w,h,rad)
                &&rounded_hit(x+0.02,y+0.98,l as f32,t as f32,w,h,rad)
                &&rounded_hit(x+0.98,y+0.98,l as f32,t as f32,w,h,rad);
            if full{fb.blend_native(px,py,color,alpha);continue;}
            let mut samples=0u16;
            for oy in [0.125,0.375,0.625,0.875]{
                for ox in [0.125,0.375,0.625,0.875]{
                    if rounded_hit(x+ox,y+oy,l as f32,t as f32,w,h,rad){samples+=1;}
                }
            }
            if samples!=0{fb.blend_native(px,py,color,((samples*u16::from(alpha)+8)/16) as u8);}
        }
    }
}
pub fn bar(fb:&mut Framebuffer,r:Rect,color:Color,alpha:u8){round(fb,r,0.0,color,alpha);}
pub fn circle(fb:&mut Framebuffer,x:i32,y:i32,r:i32,color:Color,alpha:u8){
    let (cx,cy)=fb.to_screen(x,y);
    let s=fb.desktop_scale();
    let rr=r as f32*s;
    let lim=(rr+2.0) as i32;
    let rr2=rr*rr;
    for py in cy-lim..=cy+lim {
        for px in cx-lim..=cx+lim {
            let dx=px as f32+0.5-cx as f32;
            let dy=py as f32+0.5-cy as f32;
            let d2=dx*dx+dy*dy;
            if d2<(rr-0.8)*(rr-0.8){fb.blend_native(px,py,color,alpha);}
            else if d2<(rr+0.8)*(rr+0.8){
                let mut cov=0u16;
                for oy in [0.125,0.375,0.625,0.875]{
                    for ox in [0.125,0.375,0.625,0.875]{
                        let dx=px as f32+ox-cx as f32;
                        let dy=py as f32+oy-cy as f32;
                        if dx*dx+dy*dy<rr2{cov+=1;}
                    }
                }
                if cov>0{fb.blend_native(px,py,color,((cov*u16::from(alpha)+8)/16) as u8);}
            }
        }
    }
}
pub fn text(fb:&mut Framebuffer,x:i32,y:i32,value:&str,color:Color,scale:f32,max_logical_width:i32){
    // Font size is anchored to the actual physical resolution: 21px at 1080p
    // and 42px at 2160p. No low-resolution glyph tiles are blown up.
    let (start_x,start_y)=fb.to_screen(x,y);
    let (end_x,_)=fb.to_screen(x+max_logical_width,y);
    let size=(fb.desktop_scale()/2.167).max(0.5)*scale;
    let mut pen=start_x;
    for ch in value.bytes().take(200){
        if ch<32||ch>126{continue;}
        let offset=HEADER+(ch as usize-32)*CELL;
        let advance=ATLAS[offset] as f32;
        if pen+((advance+1.0)*size) as i32>end_x{break;}
        // Bilinear alpha filtering preserves smooth font edges under DPI changes.
        let fw=(GW as f32*size) as i32+1;
        let fh=(GH as f32*size) as i32+1;
        for dy in 0..fh{
            let fy=dy as f32/size;
            let sy=(fy as usize).min(GH-1);
            let sy1=(sy+1).min(GH-1);
            let ry=fy-sy as f32;
            for dx in 0..fw{
                let fx=dx as f32/size;
                let sx=(fx as usize).min(GW-1);
                let sx1=(sx+1).min(GW-1);
                let rx=fx-sx as f32;
                let i=offset+1;
                let a00=ATLAS[i+sy*GW+sx] as f32;
                let a10=ATLAS[i+sy*GW+sx1] as f32;
                let a01=ATLAS[i+sy1*GW+sx] as f32;
                let a11=ATLAS[i+sy1*GW+sx1] as f32;
                let a=(a00*(1.0-rx)*(1.0-ry)+a10*rx*(1.0-ry)
                   +a01*(1.0-rx)*ry+a11*rx*ry+0.5) as u8;
                if a>0{fb.blend_native(pen+dx,start_y+dy,color,a);}
            }
        }
        pen+=((advance+0.5)*size) as i32;
    }
}
pub fn number(fb:&mut Framebuffer,x:i32,y:i32,mut n:u32,c:Color,scale:f32,width:i32){
    let mut buf=[0u8;12];let mut i=buf.len();
    loop{i-=1;buf[i]=b'0'+(n%10) as u8;n/=10;if n==0{break;}}
    if let Ok(s)=core::str::from_utf8(&buf[i..]){text(fb,x,y,s,c,scale,width);}
}
