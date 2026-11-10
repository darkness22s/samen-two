//! The Quite OK Image (QOI) decoder used by Samen's framebuffer shell.
#![allow(clippy::identity_op)]

/// Decode a QOI RGB image to a caller-owned buffer without allocating.
/// Supports both RGB and RGBA pixel opcodes, discarding alpha for screen output.
pub fn decode_rgb(data: &[u8], out: &mut [u8]) -> bool {
    if data.len() < 22 || &data[..4] != b"qoif" { return false; }
    let w = u32::from_be_bytes([data[4],data[5],data[6],data[7]]) as usize;
    let h = u32::from_be_bytes([data[8],data[9],data[10],data[11]]) as usize;
    if w == 0 || h == 0 || w.checked_mul(h).and_then(|n| n.checked_mul(3)) != Some(out.len())
        || !matches!(data[12], 3 | 4) || data[13] > 1
        || &data[data.len()-8..] != &[0,0,0,0,0,0,0,1] { return false; }
    let end = data.len()-8;
    let mut pos=14;
    let mut index = [[0u8; 4];64];
    let mut px = [0u8,0,0,255];
    let mut run=0usize;
    for target in out.chunks_exact_mut(3) {
        if run > 0 {
            run-=1;
        } else {
            if pos>=end { return false; }
            let op=data[pos];pos+=1;
            match op {
                0xfe => {
                    if pos+3>end{return false;}
                    px[..3].copy_from_slice(&data[pos..pos+3]);pos+=3;
                }
                0xff => {
                    if pos+4>end{return false;}
                    px.copy_from_slice(&data[pos..pos+4]);pos+=4;
                }
                _ => match op & 0xc0 {
                    0x00 => px=index[(op & 0x3f) as usize],
                    0x40 => {
                        px[0]=px[0].wrapping_add((op>>4)&3).wrapping_sub(2);
                        px[1]=px[1].wrapping_add((op>>2)&3).wrapping_sub(2);
                        px[2]=px[2].wrapping_add(op&3).wrapping_sub(2);
                    }
                    0x80 => {
                        if pos>=end{return false;}
                        let b=data[pos];pos+=1;
                        let dg=(op&63) as i16-32;
                        let dr=dg+((b>>4) as i16)-8;
                        let db=dg+((b&15) as i16)-8;
                        px[0]=((px[0] as i16)+dr) as u8;
                        px[1]=((px[1] as i16)+dg) as u8;
                        px[2]=((px[2] as i16)+db) as u8;
                    }
                    _ => {
                        run=(op&63) as usize;
                    }
                },
            }
        }
        let h=(px[0] as usize *3 +px[1] as usize*5+px[2] as usize*7+px[3] as usize*11)%64;
        index[h]=px;
        target.copy_from_slice(&px[..3]);
    }
    pos==end
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid() {
        assert!(!decode_rgb(b"", &mut [0;3]));
        let mut sample=[0u8;3];
        assert!(!decode_rgb(b"qoif", &mut sample));
    }
    #[test]
    fn decodes_exact_samen_desktop() {
        const IMAGE:&[u8]=include_bytes!("../assets/samen-desktop.qoi");
        let mut rgb=std::vec![0u8;692*498*3];
        assert!(decode_rgb(IMAGE,&mut rgb));
        assert_eq!(&rgb[0..3], &[0,53,143]); // source top-left pixel
    }
}

#[cfg(test)]
mod wallpaper_tests {
    use super::decode_rgb;
    #[test]
    fn high_resolution_wallpaper_round_trip() {
        const IMAGE:&[u8]=include_bytes!("../assets/wallpaper-full-hd.qoi");
        let mut pixels=std::vec![0u8;2048*1152*3];
        assert!(decode_rgb(IMAGE,&mut pixels));
        assert_ne!(&pixels[0..3], &[0,0,0]);
        assert_ne!(&pixels[(1151*2048+2047)*3..], &[0,0,0]);
    }
}
