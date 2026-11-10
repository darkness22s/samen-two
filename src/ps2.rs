//! Polled PS/2 controller with both mouse packets and set-1 keyboard input.
//! The i8042 provides one shared queue; never discard keyboard bytes while
//! polling the mouse. QEMU/SeaBIOS normally exposes translated set-1 scancodes.
use crate::arch::{inb,outb};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Key{Char(u8),Enter,Backspace}
#[derive(Clone,Copy)]
pub struct Motion{
    pub dx:i32,
    pub dy:i32,
    pub left_pressed:bool,
    pub left_down:bool,
}
pub enum InputEvent{Mouse(Motion),Key(Key)}
#[derive(Default)]
pub struct Mouse{
    packet:[u8;3],
    offset:usize,
    pub buttons:u8,
    shift:bool,
    caps:bool,
    extended:bool,
}
fn wait_input()->bool{
    for _ in 0..100_000{
        if unsafe{inb(0x64)}&2==0{return true;}
        core::hint::spin_loop();
    }
    false
}
fn wait_output()->bool{
    for _ in 0..100_000{
        if unsafe{inb(0x64)}&1!=0{return true;}
        core::hint::spin_loop();
    }
    false
}
fn controller(cmd:u8)->bool{
    if !wait_input(){return false;}
    unsafe{outb(0x64,cmd);}
    true
}
fn data(byte:u8)->bool{
    if !wait_input(){return false;}
    unsafe{outb(0x60,byte);}
    true
}
fn mouse_command(byte:u8)->bool{
    if !controller(0xd4)||!data(byte)||!wait_output(){return false;}
    unsafe{inb(0x60)==0xfa}
}
fn key_map(code:u8,shift:bool,caps:bool)->Option<Key>{
    match code{
        0x1c=>return Some(Key::Enter),
        0x0e=>return Some(Key::Backspace),
        _=>{}
    }
    // Keycodes are set 1 (i8042 translation enabled in init).
    const LETTERS:[(u8,u8);26]=[
        (0x1e,b'a'),(0x30,b'b'),(0x2e,b'c'),(0x20,b'd'),
        (0x12,b'e'),(0x21,b'f'),(0x22,b'g'),(0x23,b'h'),
        (0x17,b'i'),(0x24,b'j'),(0x25,b'k'),(0x26,b'l'),
        (0x32,b'm'),(0x31,b'n'),(0x18,b'o'),(0x19,b'p'),
        (0x10,b'q'),(0x13,b'r'),(0x1f,b's'),(0x14,b't'),
        (0x16,b'u'),(0x2f,b'v'),(0x11,b'w'),(0x2d,b'x'),
        (0x15,b'y'),(0x2c,b'z'),
    ];
    for (sc,c) in LETTERS {
        if sc==code{return Some(Key::Char(if shift^caps{c-32}else{c}));}
    }
    let pair=match code{
        0x02=>(b'1',b'!'),0x03=>(b'2',b'@'),0x04=>(b'3',b'#'),
        0x05=>(b'4',b'$'),0x06=>(b'5',b'%'),0x07=>(b'6',b'^'),
        0x08=>(b'7',b'&'),0x09=>(b'8',b'*'),0x0a=>(b'9',b'('),
        0x0b=>(b'0',b')'),0x0c=>(b'-',b'_'),0x0d=>(b'=',b'+'),
        0x1a=>(b'[',b'{'),0x1b=>(b']',b'}'),0x27=>(b';',b':'),
        0x28=>(b'\'',b'"'),0x29=>(b'`',b'~'),0x2b=>(b'\\',b'|'),
        0x33=>(b',',b'<'),0x34=>(b'.',b'>'),0x35=>(b'/',b'?'),
        0x39=>(b' ',b' '),
        _=>return None,
    };
    Some(Key::Char(if shift{pair.1}else{pair.0}))
}
impl Mouse{
    pub fn init()->Option<Self>{
        for _ in 0..32{
            if unsafe{inb(0x64)}&1==0{break;}
            unsafe{inb(0x60);}
        }
        if !controller(0xae)||!controller(0xa8){return None;}
        if !controller(0x20)||!wait_output(){return None;}
        let config=unsafe{inb(0x60)};
        // Poll both keyboard and mouse. Translate the keyboard to set 1.
        if !controller(0x60)||!data((config & !0x23)|0x40){return None;}
        if !mouse_command(0xf6)||!mouse_command(0xf4){return None;}
        // Enable keyboard scanning (ACK must be drained, otherwise it could
        // appear as a character). Mouse remains usable without this ACK.
        if data(0xf4)&&wait_output(){
            let _=unsafe{inb(0x60)};
        }
        Some(Self::default())
    }
    fn key(&mut self,byte:u8)->Option<Key>{
        if byte==0xe0{self.extended=true;return None;}
        if self.extended{self.extended=false;return None;}
        let code=byte&0x7f;
        let released=byte&0x80!=0;
        if code==0x2a||code==0x36{self.shift=!released;return None;}
        if code==0x3a&&!released{self.caps=!self.caps;return None;}
        if released{return None;}
        key_map(code,self.shift,self.caps)
    }
    pub fn poll(&mut self)->Option<InputEvent>{
        let status=unsafe{inb(0x64)};
        if status&1==0{return None;}
        let byte=unsafe{inb(0x60)};
        if status&0x20==0{return self.key(byte).map(InputEvent::Key);}
        if self.offset==0&&byte&0x08==0{return None;}
        self.packet[self.offset]=byte;
        self.offset+=1;
        if self.offset!=3{return None;}
        self.offset=0;
        let [flags,x,y]=self.packet;
        if flags&0xc0!=0{return None;}
        let left=flags&1!=0;
        let pressed=left&&self.buttons&1==0;
        self.buttons=flags&7;
        Some(InputEvent::Mouse(Motion{
            dx:(x as i8) as i32,
            dy:-((y as i8) as i32),
            left_pressed:pressed,
            left_down:left,
        }))
    }
}
