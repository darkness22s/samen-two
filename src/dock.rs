//! Native-pixel, subpixel-antialiased dock. All geometry is evaluated at
//! framebuffer resolution. The old 692x498 grid is ONLY used for placement;
//! no low-resolution bitmap or logical_blend() calls occur in this module.
//!
//! The CPU compositor uses a 4x4 coverage mask at every raster edge. Icons
//! are vector geometry rasterized at the device pixel size (1080p, 1440p,
//! 4K, etc.), not an upscaled screenshot or tiled logical pixels.
use crate::gui::{App, Framebuffer};

const SAMPLES: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
type Color = [u8;3];

#[derive(Clone,Copy)]
struct Area { x:i32, y:i32, w:i32, h:i32 }
impl Area {
    fn around(cx:f32,cy:f32,r:f32)->Self{
        let pad=r as i32+3;
        Self{x:cx as i32-pad,y:cy as i32-pad,w:pad*2+6,h:pad*2+6}
    }
}

// The closure defines an analytic shape in native-screen pixel coordinates.
// Four-by-four subpixel coverage turns sharp vector edges into smooth ramps.
fn raster<F>(fb:&mut Framebuffer, area:Area, color:Color, opacity:u8, contains:F)
where F:Fn(f32,f32)->bool {
    if opacity==0{return;}
    let (w,h)=fb.dimensions();
    let min_x=area.x.max(0);
    let min_y=area.y.max(0);
    let max_x=area.x.saturating_add(area.w).min(w as i32);
    let max_y=area.y.saturating_add(area.h).min(h as i32);
    for y in min_y..max_y{
        let py=y as f32;
        for x in min_x..max_x{
            let px=x as f32;
            let mut covered=0u32;
            for fy in SAMPLES{
                for fx in SAMPLES{
                    if contains(px+fx,py+fy){covered+=1;}
                }
            }
            if covered>0{
                let alpha=((covered*u32::from(opacity)+8)/16) as u8;
                fb.blend_native(x,y,color,alpha);
            }
        }
    }
}
#[inline(always)]
fn within_circle(x:f32,y:f32,cx:f32,cy:f32,r:f32)->bool{
    let dx=x-cx;
    let dy=y-cy;
    dx*dx+dy*dy<=r*r
}
#[inline(always)]
fn within_round(x:f32,y:f32,l:f32,t:f32,w:f32,h:f32,r:f32)->bool{
    if x<l||y<t||x>=l+w||y>=t+h{return false;}
    let r=r.min(w*0.5).min(h*0.5);
    let dx=if x<l+r{l+r-x}else if x>l+w-r{x-(l+w-r)}else{0.0};
    let dy=if y<t+r{t+r-y}else if y>t+h-r{y-(t+h-r)}else{0.0};
    dx*dx+dy*dy<=r*r
}
fn round(fb:&mut Framebuffer,l:f32,t:f32,w:f32,h:f32,r:f32,c:Color,a:u8){
    let area=Area{x:l as i32-2,y:t as i32-2,w:w as i32+5,h:h as i32+5};
    raster(fb,area,c,a,|x,y|within_round(x,y,l,t,w,h,r));
}
fn disc(fb:&mut Framebuffer,cx:f32,cy:f32,r:f32,c:Color,a:u8){
    raster(fb,Area::around(cx,cy,r),c,a,|x,y|within_circle(x,y,cx,cy,r));
}
fn clip_arc(fb:&mut Framebuffer,cx:f32,cy:f32,r:f32,inner:f32,c:Color,a:u8){
    raster(fb,Area::around(cx,cy,r),c,a,|x,y|{
        within_circle(x,y,cx,cy,r) && !within_circle(x,y,cx,cy,inner)
    });
}

fn chrome(fb:&mut Framebuffer,cx:f32,cy:f32,s:f32){
    let radius=16.5*s;
    // Chrome's three curved color blades, each coverage-sampled at physical
    // pixel resolution. Their center is covered by the white/blue hub.
    raster(fb,Area::around(cx,cy,radius),[53,162,81],255,|x,y|{
        within_circle(x,y,cx,cy,radius)
    });
    raster(fb,Area::around(cx,cy,radius),[235,67,63],255,|x,y|{
        let dx=x-cx;let dy=y-cy;
        within_circle(x,y,cx,cy,radius) &&
        dy< -0.57*dx && dy<0.58*dx+2.4*s
    });
    raster(fb,Area::around(cx,cy,radius),[251,194,43],255,|x,y|{
        let dx=x-cx;let dy=y-cy;
        within_circle(x,y,cx,cy,radius) &&
        dx>0.0 && dy>= -0.57*dx && dy>0.57*dx-4.0*s
    });
    // The neutral separator and blue core are concentric and genuinely
    // circular even on an HD screen.
    disc(fb,cx,cy,9.9*s,[244,248,255],255);
    disc(fb,cx,cy,7.9*s,[46,140,231],255);
    disc(fb,cx-3.2*s,cy-3.4*s,1.45*s,[159,216,255],197);
}

fn files(fb:&mut Framebuffer,cx:f32,cy:f32,s:f32){
    // Back plate, title tab, lower panel and a deliberately subtle highlight.
    // All radii use hardware pixels; the old 35x23 icon raster is gone.
    round(fb,cx-16.0*s,cy-12.0*s,18.0*s,10.0*s,3.2*s,[53,145,217],255);
    round(fb,cx-17.1*s,cy-8.4*s,34.5*s,24.2*s,4.2*s,[34,132,213],255);
    round(fb,cx-16.2*s,cy-5.9*s,32.3*s,21.1*s,4.3*s,[92,190,247],255);
    round(fb,cx-14.0*s,cy-4.0*s,28.1*s,3.2*s,1.3*s,[193,233,255],186);
    round(fb,cx-14.0*s,cy+12.9*s,28.1*s,1.5*s,0.7*s,[36,134,212],95);
}

fn cog(fb:&mut Framebuffer,cx:f32,cy:f32,s:f32){
    // Native vector gear: an outer tile, eight rectangular teeth, a clean
    // circular ring and a central bore with its own specular glint.
    round(fb,cx-18.0*s,cy-18.0*s,36.0*s,36.0*s,8.2*s,[38,52,73],255);
    round(fb,cx-16.5*s,cy-16.5*s,33.0*s,33.0*s,7.4*s,[73,91,115],255);
    round(fb,cx-14.0*s,cy-14.3*s,28.0*s,5.0*s,2.5*s,[162,180,202],53);
    // Eight directions defined by unit vectors: fast enough for MSAA and
    // doesn't depend on trigonometric functions in the freestanding kernel.
    const DIRS:[(f32,f32);8]=[
        (1.0,0.0),(0.70710677,0.70710677),(0.0,1.0),
        (-0.70710677,0.70710677),(-1.0,0.0),
        (-0.70710677,-0.70710677),(0.0,-1.0),
        (0.70710677,-0.70710677),
    ];
    let outer=13.4*s;
    raster(fb,Area::around(cx,cy,outer),[242,246,253],255,|x,y|{
        let dx=x-cx;let dy=y-cy;let d2=dx*dx+dy*dy;
        if d2<=11.1*s*11.1*s && d2>=7.8*s*7.8*s{return true;}
        if d2>outer*outer{return false;}
        DIRS.iter().any(|&(ux,uy)|{
            let along=dx*ux+dy*uy;
            let across=dx*(-uy)+dy*ux;
            along>=9.0*s && along<=13.4*s && across>=-2.1*s && across<=2.1*s
        })
    });
    disc(fb,cx,cy,5.1*s,[69,89,117],255);
    disc(fb,cx-1.3*s,cy-1.4*s,1.15*s,[140,168,195],121);
}

pub fn draw(fb:&mut Framebuffer,selected:Option<App>) {
    let s=fb.desktop_scale();
    let (px,py)=fb.to_screen(260,417);
    let x=px as f32;
    let y=py as f32;
    let w=194.0*s;
    let h=55.0*s;
    let r=17.0*s;

    // Shadow layers with proper antialiased corner coverage.
    round(fb,x-5.0*s,y+6.7*s,w+10.0*s,h+5.0*s,r+4.0*s,[0,21,60],21);
    round(fb,x-2.2*s,y+3.8*s,w+4.4*s,h+2.0*s,r+1.5*s,[5,29,67],40);
    // Outer light rim and independent frosted glass plate.
    round(fb,x-0.7*s,y-0.7*s,w+1.4*s,h+1.4*s,r+0.7*s,[248,251,255],89);
    round(fb,x,y,w,h,r,[228,237,250],195);
    // Shine at the upper glass lip. The translucent tint lets real wallpaper
    // colors show through rather than painting a screenshot.
    round(fb,x+2.0*s,y+1.0*s,w-4.0*s,1.4*s,0.7*s,[255,255,255],143);
    round(fb,x+5.0*s,y+h-3.3*s,w-10.0*s,1.1*s,0.6*s,[91,126,172],43);

    for (app,logical_x) in [
        (App::Browser,309),
        (App::Files,357),
        (App::Settings,406),
    ] {
        let (ix,iy)=fb.to_screen(logical_x,445);
        let cx=ix as f32;
        let cy=iy as f32;
        disc(fb,cx,cy+2.5*s,22.0*s,[20,37,72],62);
        disc(fb,cx,cy,21.0*s,[239,245,253],223);
        if selected==Some(app) {
            disc(fb,cx,cy,19.0*s,[192,218,253],208);
            clip_arc(fb,cx,cy,20.5*s,19.2*s,[249,252,255],218);
        } else {
            clip_arc(fb,cx,cy,20.0*s,19.35*s,[255,255,255],144);
        }
        match app {
            App::Browser=>chrome(fb,cx,cy,s),
            App::Files=>files(fb,cx,cy,s),
            App::Settings=>cog(fb,cx,cy,s),
        }
    }
}
