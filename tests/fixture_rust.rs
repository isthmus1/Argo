#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusWidth {
    Bus8 = 1,
    Bus16 = 2,
    Bus32 = 4,
    Bus64 = 8,
}

pub struct Ram {
    pub data: *mut u8,
    pub size: usize,
    pub base: usize,
}

fn mask_for(w: BusWidth) -> usize {
    w as usize - 1
}

pub fn ram_read(r: &Ram, addr: usize, w: BusWidth, out: &mut usize) -> i32 {
    if r.data.is_null() {
        return -1;
    }
    let offset = addr - r.base;
    if offset >= r.size {
        return -1;
    }
    *out = 0;
    match w {
        BusWidth::Bus8 => {
            *out = r.data[offset] as usize;
        }
        BusWidth::Bus16 => {
            *out = r.data[offset] as usize | ((r.data[offset + 1] as usize) << 8);
        }
        _ => {
            return -2;
        }
    }
    0
}

pub fn ram_fill(r: &Ram, v: u8) {
    let mut i = 0usize;
    while i < r.size {
        unsafe {
            *r.data.add(i) = v;
        }
        i += 1;
    }
    while i > 0 {
        i -= 1;
    }
    mask_for(BusWidth::Bus32);
}
