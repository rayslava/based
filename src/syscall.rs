#![allow(dead_code)]

// Constants for system calls
pub const READ: usize = 0;
pub const WRITE: usize = 1;
pub const EXIT: usize = 60;
pub const IOCTL: usize = 16;

// File descriptors
pub const STDIN: usize = 0;
pub const STDOUT: usize = 1;

// Max error value for syscalls (typically -4095 to -1 in Linux)
// So the "wrapped" values would be from MAX-4095 to MAX
const MAX_ERRNO: usize = 4095;

// Result type for syscalls
pub type SysResult = Result<usize, usize>;

// Check if a syscall result is an error
#[inline]
fn is_error(result: usize) -> bool {
    result > usize::MAX - MAX_ERRNO
}

// Exit function
pub fn exit(status: usize) -> ! {
    unsafe {
        syscall!(EXIT, status);
        core::hint::unreachable_unchecked()
    }
}

// Write function
pub fn write(fd: usize, buf: &[u8]) -> SysResult {
    let result = unsafe { syscall!(WRITE, fd, buf.as_ptr(), buf.len()) };
    if !is_error(result) {
        Ok(result)
    } else {
        Err(usize::MAX - result + 1) // Extract actual errno
    }
}

#[cfg(not(test))]
// Read function
pub fn read(fd: usize, buf: &mut [u8], count: usize) -> SysResult {
    let result = unsafe { syscall!(READ, fd, buf.as_ptr(), count) };
    if !is_error(result) {
        Ok(result)
    } else {
        Err(usize::MAX - result + 1) // Extract actual errno
    }
}

#[cfg(test)]
pub fn read(_fd: usize, buf: &mut [u8], _count: usize) -> SysResult {
    use crate::editor::tests::TEST_INPUT;

    TEST_INPUT.with(|inputs| {
        let mut inputs = inputs.borrow_mut();
        if let Some(input) = inputs.pop_front() {
            if !buf.is_empty() {
                buf[0] = input;
                return Ok(1);
            }
        }
        Ok(0)
    })
}

// ioctl function
pub fn ioctl(fd: usize, request: usize, arg: usize) -> SysResult {
    let result = unsafe { syscall!(IOCTL, fd, request, arg) };
    if !is_error(result) {
        Ok(result)
    } else {
        Err(usize::MAX - result + 1) // Extract actual errno
    }
}

#[cfg(not(test))]
pub fn puts(msg: &[u8]) -> SysResult {
    write(STDOUT, msg)
}

#[cfg(test)]
pub fn puts(msg: &[u8]) -> SysResult {
    use crate::terminal::tests::handle_test_puts;
    handle_test_puts(msg)
}

// Write a single byte
pub fn putchar(c: u8) -> SysResult {
    let buf = [c];
    write(STDOUT, &buf)
}
