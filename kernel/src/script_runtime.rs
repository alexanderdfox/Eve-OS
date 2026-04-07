// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Small in-house script VM scaffold for future web scripting support.

#[derive(Clone, Copy)]
pub enum Op {
    PushI32(i32),
    Add,
    Sub,
    Mul,
    Div,
    CallHost(u8),
    Ret,
}

pub struct Program<'a> {
    pub code: &'a [Op],
}

pub struct ByteProgram<'a> {
    pub code: &'a [u8],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    BadOpcode,
    TruncatedImm,
    StepLimit,
}

#[derive(Clone, Copy)]
pub enum HostCall {
    TimerSet = 0,
    FetchStart = 1,
    StorageGet = 2,
    StorageSet = 3,
}

pub trait HostApi {
    fn timer_set(&mut self, _ms: u32) -> i32 {
        -1
    }
    fn fetch_start(&mut self, _url_id: i32) -> i32 {
        -1
    }
    fn storage_get(&mut self, _key_id: i32) -> i32 {
        -1
    }
    fn storage_set(&mut self, _key_id: i32, _value_id: i32) -> i32 {
        -1
    }
}

pub struct NullHost;
impl HostApi for NullHost {}

pub struct Vm {
    stack: [i32; 64],
    sp: usize,
    pc: usize,
    step_limit: usize,
}

impl Vm {
    pub const fn new() -> Self {
        Self {
            stack: [0; 64],
            sp: 0,
            pc: 0,
            step_limit: 4096,
        }
    }

    pub fn reset(&mut self) {
        self.sp = 0;
        self.pc = 0;
    }

    pub fn set_step_limit(&mut self, n: usize) {
        self.step_limit = n.max(1);
    }

    pub fn run<H: HostApi>(&mut self, p: Program<'_>, host: &mut H) -> i32 {
        self.reset();
        let mut steps = 0usize;
        while self.pc < p.code.len() {
            steps = steps.saturating_add(1);
            if steps > self.step_limit {
                return 0;
            }
            let op = p.code[self.pc];
            self.pc += 1;
            match op {
                Op::PushI32(v) => self.push(v),
                Op::Add => binop(self, |a, b| a.wrapping_add(b)),
                Op::Sub => binop(self, |a, b| a.wrapping_sub(b)),
                Op::Mul => binop(self, |a, b| a.wrapping_mul(b)),
                Op::Div => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(if b == 0 { 0 } else { a.wrapping_div(b) });
                }
                Op::CallHost(id) => {
                    let ret = match id {
                        x if x == HostCall::TimerSet as u8 => {
                            let ms = self.pop().max(0) as u32;
                            host.timer_set(ms)
                        }
                        x if x == HostCall::FetchStart as u8 => {
                            let url_id = self.pop();
                            host.fetch_start(url_id)
                        }
                        x if x == HostCall::StorageGet as u8 => {
                            let key_id = self.pop();
                            host.storage_get(key_id)
                        }
                        x if x == HostCall::StorageSet as u8 => {
                            let value_id = self.pop();
                            let key_id = self.pop();
                            host.storage_set(key_id, value_id)
                        }
                        _ => -1,
                    };
                    self.push(ret);
                }
                Op::Ret => return self.pop(),
            }
        }
        0
    }

    pub fn run_bytes<H: HostApi>(
        &mut self,
        p: ByteProgram<'_>,
        host: &mut H,
    ) -> Result<i32, VmError> {
        self.reset();
        let mut steps = 0usize;
        while self.pc < p.code.len() {
            steps = steps.saturating_add(1);
            if steps > self.step_limit {
                return Err(VmError::StepLimit);
            }
            let op = decode_op(p.code, &mut self.pc)?;
            if let Some(ret) = self.exec_one(op, host) {
                return Ok(ret);
            }
        }
        Ok(0)
    }

    fn exec_one<H: HostApi>(&mut self, op: Op, host: &mut H) -> Option<i32> {
        match op {
            Op::PushI32(v) => self.push(v),
            Op::Add => binop(self, |a, b| a.wrapping_add(b)),
            Op::Sub => binop(self, |a, b| a.wrapping_sub(b)),
            Op::Mul => binop(self, |a, b| a.wrapping_mul(b)),
            Op::Div => {
                let b = self.pop();
                let a = self.pop();
                self.push(if b == 0 { 0 } else { a.wrapping_div(b) });
            }
            Op::CallHost(id) => {
                let ret = match id {
                    x if x == HostCall::TimerSet as u8 => {
                        let ms = self.pop().max(0) as u32;
                        host.timer_set(ms)
                    }
                    x if x == HostCall::FetchStart as u8 => {
                        let url_id = self.pop();
                        host.fetch_start(url_id)
                    }
                    x if x == HostCall::StorageGet as u8 => {
                        let key_id = self.pop();
                        host.storage_get(key_id)
                    }
                    x if x == HostCall::StorageSet as u8 => {
                        let value_id = self.pop();
                        let key_id = self.pop();
                        host.storage_set(key_id, value_id)
                    }
                    _ => -1,
                };
                self.push(ret);
            }
            Op::Ret => return Some(self.pop()),
        }
        None
    }

    fn push(&mut self, v: i32) {
        if self.sp < self.stack.len() {
            self.stack[self.sp] = v;
            self.sp += 1;
        }
    }

    fn pop(&mut self) -> i32 {
        if self.sp == 0 {
            return 0;
        }
        self.sp -= 1;
        self.stack[self.sp]
    }
}

fn binop(vm: &mut Vm, f: impl FnOnce(i32, i32) -> i32) {
    let b = vm.pop();
    let a = vm.pop();
    vm.push(f(a, b));
}

fn decode_op(code: &[u8], pc: &mut usize) -> Result<Op, VmError> {
    if *pc >= code.len() {
        return Err(VmError::BadOpcode);
    }
    let op = code[*pc];
    *pc += 1;
    match op {
        0x01 => {
            if pc.saturating_add(4) > code.len() {
                return Err(VmError::TruncatedImm);
            }
            let b0 = code[*pc];
            let b1 = code[*pc + 1];
            let b2 = code[*pc + 2];
            let b3 = code[*pc + 3];
            *pc += 4;
            Ok(Op::PushI32(i32::from_le_bytes([b0, b1, b2, b3])))
        }
        0x02 => Ok(Op::Add),
        0x03 => Ok(Op::Sub),
        0x04 => Ok(Op::Mul),
        0x05 => Ok(Op::Div),
        0x06 => {
            if *pc >= code.len() {
                return Err(VmError::TruncatedImm);
            }
            let id = code[*pc];
            *pc += 1;
            Ok(Op::CallHost(id))
        }
        0x07 => Ok(Op::Ret),
        _ => Err(VmError::BadOpcode),
    }
}

#[inline]
fn lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() {
        b + 32
    } else {
        b
    }
}

fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    'outer: for i in 0..=hay.len() - needle.len() {
        for j in 0..needle.len() {
            if lower(hay[i + j]) != lower(needle[j]) {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

#[inline]
fn hex_nybble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Execute one `eve-script:` inline bytecode marker from page bytes.
///
/// Marker format:
/// - ASCII `eve-script:`
/// - then contiguous hex pairs (e.g. `010100000007` for PUSH 1; RET)
pub fn run_page_eve_script(raw: &[u8], enabled: bool) -> Option<Result<i32, VmError>> {
    if !enabled {
        return None;
    }
    let marker = b"eve-script:";
    let at = find_ci(raw, marker)?;
    let mut i = at + marker.len();
    while i < raw.len() && matches!(raw[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    let mut code = [0u8; 256];
    let mut len = 0usize;
    while i + 1 < raw.len() && len < code.len() {
        let Some(hi) = hex_nybble(raw[i]) else {
            break;
        };
        let Some(lo) = hex_nybble(raw[i + 1]) else {
            break;
        };
        code[len] = (hi << 4) | lo;
        len += 1;
        i += 2;
    }
    if len == 0 {
        return Some(Err(VmError::TruncatedImm));
    }
    let mut vm = Vm::new();
    let mut host = NullHost;
    Some(vm.run_bytes(ByteProgram { code: &code[..len] }, &mut host))
}
