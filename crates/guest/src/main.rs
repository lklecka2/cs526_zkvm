#![no_main]

use std::alloc::{alloc, dealloc, Layout};
use std::mem;

risc0_zkvm::guest::entry!(main);

extern "C" {
    fn cmain() -> i32;
}

#[no_mangle]
pub extern "C" fn malloc(size: u32) -> *mut u8 {
    unsafe {
        let header = mem::size_of::<usize>();
        let user_size = size as usize;
        let total = header.checked_add(user_size).expect("malloc size overflow");
        let layout = Layout::from_size_align(total, mem::align_of::<usize>()).unwrap();
        let raw = alloc(layout);
        if raw.is_null() {
            panic!("malloc failed");
        }
        (raw as *mut usize).write(total);
        raw.add(header)
    }
}

#[no_mangle]
pub extern "C" fn free(ptr: *mut u8) {
    unsafe {
        if ptr.is_null() {
            return;
        }
        let header = mem::size_of::<usize>();
        let raw = ptr.sub(header);
        let total = (raw as *mut usize).read();
        let layout = Layout::from_size_align(total, mem::align_of::<usize>()).unwrap();
        dealloc(raw, layout);
    }
}

fn main() {
    let result = unsafe { cmain() };
    risc0_zkvm::guest::env::commit(&result);
}
