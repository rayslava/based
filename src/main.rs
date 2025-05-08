#![deny(warnings)]
#![cfg_attr(test, allow(unused_imports))]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[macro_use]
extern crate sc;

mod editor;
mod syscall;
mod terminal;
mod termios;

use editor::run_editor;
use syscall::{exit, puts};
use terminal::{get_termios, get_winsize, set_raw_mode, set_termios};
use termios::{TCSETS, TCSETSW, Termios, Winsize};

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}

/// # Safety
///
/// No one is safe once this program is started.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() {
    // Need to align stack to make sse work
    unsafe { core::arch::asm!("and rsp, -64", options(nomem, nostack)) };

    match main() {
        Ok(()) => exit(0),
        Err(e) => exit(e),
    }
}

fn main() -> Result<(), usize> {
    let mut winsize = Winsize::new();

    if get_winsize(syscall::STDOUT, &mut winsize).is_ok() {
        puts(b"Terminal size: ")?;
        terminal::write_number(winsize.rows);
        puts(b"x")?;
        terminal::write_number(winsize.cols);
        puts(b"\r\n")?;
    } else {
        puts(b"Could not get terminal size\r\n")?;
    }

    // Get and save original terminal settings
    let mut orig_termios = Termios::new();

    if get_termios(syscall::STDIN, &mut orig_termios).is_ok() {
        // Make a copy for raw mode
        let mut raw_termios = orig_termios;

        // Set raw mode flags
        set_raw_mode(&mut raw_termios);

        // Apply raw mode
        if set_termios(syscall::STDIN, TCSETS, &raw_termios).is_ok() {
            puts(b"Entered raw mode. Press q to exit.\r\n")?;

            // Run the editor
            run_editor()?;

            // Restore original settings
            set_termios(syscall::STDIN, TCSETSW, &orig_termios)?;
            puts(b"\r\nExited raw mode\r\n")?;
        } else {
            puts(b"Failed to set raw mode\r\n")?;
        }
    } else {
        puts(b"Failed to get terminal attributes\r\n")?;
    }

    Ok(())
}
