//! Static ring-0 bump allocator for the once-allocated wallpaper and
//! two framebuffer surfaces. No heap activity occurs during mouse motion.
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, Ordering};

const BYTES: usize = 96 * 1024 * 1024;
#[repr(align(4096))]
struct Storage([u8; BYTES]);
static mut STORAGE:Storage=Storage([0;BYTES]);
static NEXT:AtomicUsize=AtomicUsize::new(0);
struct BootHeap;

unsafe impl GlobalAlloc for BootHeap {
    unsafe fn alloc(&self,layout:Layout)->*mut u8 {
        let mut at=NEXT.load(Ordering::Relaxed);
        loop {
            let Some(start)=at.checked_add(layout.align()-1)
               .map(|v|v & !(layout.align()-1)) else{return null_mut()};
            let Some(end)=start.checked_add(layout.size().max(1)) else{return null_mut()};
            if end>BYTES{return null_mut();}
            match NEXT.compare_exchange_weak(at,end,Ordering::SeqCst,Ordering::Relaxed){
                Ok(_)=>{
                    let base=unsafe{core::ptr::addr_of_mut!(STORAGE.0).cast::<u8>()};
                    return unsafe{base.add(start)};
                }
                Err(other)=>at=other,
            }
        }
    }
    unsafe fn dealloc(&self,_ptr:*mut u8,_layout:Layout){
        // Lifelong renderer buffers; the kernel doesn't recycle them.
    }
}
#[global_allocator]
static GLOBAL:BootHeap=BootHeap;
