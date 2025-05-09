#![allow(dead_code)]

// Constants for system calls
pub const READ: usize = 0;
pub const WRITE: usize = 1;
pub const OPEN: usize = 2;
pub const CLOSE: usize = 3;
pub const EXIT: usize = 60;
pub const IOCTL: usize = 16;
pub const MMAP: usize = 9;

// File descriptors
pub const STDIN: usize = 0;
pub const STDOUT: usize = 1;

// Flag constants for open
pub const O_RDONLY: usize = 0;

// Flag constants for mmap
pub const PROT_READ: usize = 1;
pub const MAP_PRIVATE: usize = 2;

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
    if is_error(result) {
        Err(usize::MAX - result + 1) // Extract actual errno
    } else {
        Ok(result)
    }
}

#[cfg(not(test))]
// Read function
pub fn read(fd: usize, buf: &mut [u8], count: usize) -> SysResult {
    let result = unsafe { syscall!(READ, fd, buf.as_ptr(), count) };
    if is_error(result) {
        Err(usize::MAX - result + 1) // Extract actual errno
    } else {
        Ok(result)
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
    if is_error(result) {
        Err(usize::MAX - result + 1) // Extract actual errno
    } else {
        Ok(result)
    }
}

#[cfg(not(test))]
pub fn write_buf(buf: &[u8]) -> SysResult {
    write(STDOUT, buf)
}

// Need to match the signature
#[allow(clippy::unnecessary_wraps)]
#[cfg(test)]
pub fn write_buf(buf: &[u8]) -> SysResult {
    use crate::terminal::tests::handle_test_puts;
    Ok(handle_test_puts(buf))
}

#[cfg(not(test))]
pub fn puts(msg: &str) -> SysResult {
    write(STDOUT, msg.as_bytes())
}

// Need to match the signature
#[allow(clippy::unnecessary_wraps)]
#[cfg(test)]
pub fn puts(msg: &str) -> SysResult {
    use crate::terminal::tests::handle_test_puts;
    Ok(handle_test_puts(msg.as_bytes()))
}

// Write a single byte
pub fn putchar(c: u8) -> SysResult {
    let buf = [c];
    write(STDOUT, &buf)
}

// Open file function
pub fn open(path: &[u8], flags: usize) -> SysResult {
    let result = unsafe { syscall!(OPEN, path.as_ptr(), flags, 0o666) };
    if is_error(result) {
        Err(usize::MAX - result + 1) // Extract actual errno
    } else {
        Ok(result)
    }
}

// Close file function
pub fn close(fd: usize) -> SysResult {
    let result = unsafe { syscall!(CLOSE, fd) };
    if is_error(result) {
        Err(usize::MAX - result + 1) // Extract actual errno
    } else {
        Ok(result)
    }
}

// Memory map function
pub fn mmap(
    addr: usize,
    length: usize,
    prot: usize,
    flags: usize,
    fd: usize,
    offset: usize,
) -> SysResult {
    let result = unsafe { syscall!(MMAP, addr, length, prot, flags, fd, offset) };
    if is_error(result) {
        Err(usize::MAX - result + 1) // Extract actual errno
    } else {
        Ok(result)
    }
}
