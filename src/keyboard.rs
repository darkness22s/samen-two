//! US-layout PS/2 set-1 scancode decoder for the native Terminal window.
//! Typing goes directly to Samen's in-kernel command interpreter.
#[derive(Clone,Copy,PartialEq,Eq,Debug)]
pub enum Key {Char(u8),Backspace,Enter}
pub struct Decoder { shift:bool, extended:bool,caps:bool }
impl Decoder {
    pub const fn new()->Self{Self{shift:false,extended:false,caps:false}}
    pub fn feed(&mut self,scan:u8)->Option<Key> {
        if scan==0xe0 {self.extended=true;return None;}
        if self.extended {self.extended=false;return None;}
        if scan==0x2a||scan==0x36 {self.shift=true;return None;}
        if scan==0xaa||scan==0xb6 {self.shift=false;return None;}
        if scan==0x3a {self.caps=!self.caps;return None;}
        if scan&0x80!=0{return None;}
        if scan==0x1c{return Some(Key::Enter)}
        if scan==0x0e{return Some(Key::Backspace)}
        const LOWER:[u8;128]=[
            0,27,b'1',b'2',b'3',b'4',b'5',b'6',b'7',b'8',b'9',b'0',b'-',b'=',0,0,
            b'q',b'w',b'e',b'r',b't',b'y',b'u',b'i',b'o',b'p',b'[',b']',0,0,b'a',b's',
            b'd',b'f',b'g',b'h',b'j',b'k',b'l',b';',b'\'',b'`',0,b'\\',b'z',b'x',b'c',b'v',
            b'b',b'n',b'm',b',',b'.',b'/',0,b'*',0,b' ',0,0,0,0,0,
            0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
            0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
            0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
            0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
        ];
        const SHIFTED:[u8;14]=[0,0,b'!',b'@',b'#',b'$',b'%',b'^',b'&',b'*',b'(',b')',b'_',b'+'];
        let mut b=*LOWER.get(scan as usize)?;
        if b==0{return None;}
        if b.is_ascii_lowercase() {
            if self.shift^self.caps {b=b.to_ascii_uppercase()}
        } else if self.shift {
            if (scan as usize)<SHIFTED.len() {b=SHIFTED[scan as usize]}
            else {b=match b {
                b'['=>b'{',b']'=>b'}',b';'=>b':',b'\''=>b'"',
                b'`'=>b'~',b'\\'=>b'|',b','=>b'<',b'.'=>b'>',
                b'/'=>b'?',other=>other
            }}
        }
        Some(Key::Char(b))
    }
}
