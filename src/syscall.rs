#![allow(dead_code)]

pub const MAX_PATH: usize = 256;

pub const READ: usize = 0;
pub const WRITE: usize = 1;
pub const OPEN: usize = 2;
pub const CLOSE: usize = 3;
pub const EXIT: usize = 60;
pub const IOCTL: usize = 16;
pub const MMAP: usize = 9;
pub const MUNMAP: usize = 11;
pub const LSEEK: usize = 8;

pub const SEEK_SET: usize = 0;
pub const SEEK_CUR: usize = 1;
pub const SEEK_END: usize = 2;

pub const STDIN: usize = 0;
pub const STDOUT: usize = 1;

pub const O_RDONLY: usize = 0;
pub const O_WRONLY: usize = 1;
pub const O_CREAT: usize = 64;
pub const O_TRUNC: usize = 512;

pub const PROT_READ: usize = 1;
pub const PROT_WRITE: usize = 2;
pub const MAP_PRIVATE: usize = 2;
pub const MAP_ANONYMOUS: usize = 0x20;

const MAX_ERRNO: usize = 4095;

pub type SysResult = Result<usize, usize>;

#[inline]
fn is_error(result: usize) -> bool {
    result > usize::MAX - MAX_ERRNO
}

#[inline]
fn syscall_result(result: usize) -> SysResult {
    if is_error(result) {
        Err(usize::MAX - result + 1)
    } else {
        Ok(result)
    }
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
    syscall_result(result)
}

// Writing the data from the raw pointer
pub fn write_unchecked(fd: usize, ptr: *const u8, len: usize) -> SysResult {
    let result = unsafe { syscall!(WRITE, fd, ptr, len) };
    syscall_result(result)
}

#[cfg(not(test))]
// Read function
pub fn read(fd: usize, buf: &mut [u8], count: usize) -> SysResult {
    let result = unsafe { syscall!(READ, fd, buf.as_ptr(), count) };
    syscall_result(result)
}

// Need to match the signature
#[allow(clippy::unnecessary_wraps)]
#[cfg(test)]
pub fn read(_fd: usize, buf: &mut [u8], _count: usize) -> SysResult {
    // Simplified test implementation that works even without std::collections
    if !buf.is_empty() {
        buf[0] = b'a';
        return Ok(1);
    }
    Ok(0)
}

// ioctl function
pub fn ioctl(fd: usize, request: usize, arg: usize) -> SysResult {
    let result = unsafe { syscall!(IOCTL, fd, request, arg) };
    syscall_result(result)
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
    syscall_result(result)
}

// Close file function
pub fn close(fd: usize) -> SysResult {
    let result = unsafe { syscall!(CLOSE, fd) };
    syscall_result(result)
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
    syscall_result(result)
}

// Memory unmap function
pub fn munmap(addr: usize, length: usize) -> SysResult {
    let result = unsafe { syscall!(MUNMAP, addr, length) };
    syscall_result(result)
}

// Seek function
pub fn lseek(fd: usize, offset: usize, whence: usize) -> SysResult {
    let result = unsafe { syscall!(LSEEK, fd, offset, whence) };
    syscall_result(result)
}
