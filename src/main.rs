#![deny(warnings)]
#![cfg_attr(test, allow(unused_imports))]
#![cfg_attr(all(not(test), not(debug_assertions)), no_std)]
#![cfg_attr(all(not(test), not(debug_assertions)), no_main)]
#[macro_use]
extern crate sc;

mod editor;
mod syscall;
mod terminal;
mod termios;

use editor::run_editor;
use syscall::{exit, puts};
use terminal::{get_termios, get_winsize, set_raw_mode, set_termios, write_number};
use termios::{TCSETS, TCSETSW, Termios, Winsize};

#[cfg(all(not(test), not(debug_assertions)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}

/// # Safety
///
/// No one is safe once this program is started.
#[cfg(all(not(test), not(debug_assertions)))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    // Need to align stack to make sse work
    unsafe { core::arch::asm!("and rsp, -64", options(nomem, nostack)) };

    match run() {
        Ok(()) => exit(0),
        Err(e) => exit(e),
    }
}

#[cfg(debug_assertions)]
fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => exit(e),
    }
}

fn run() -> Result<(), usize> {
    let mut winsize = Winsize::new();

    if get_winsize(syscall::STDOUT, &mut winsize).is_ok() {
        puts("Ready to open file using mmap\r\n")?;
    } else {
        puts("Could not get terminal size\r\n")?;
    }

    // Get and save original terminal settings
    let mut orig_termios = Termios::new();

    if get_termios(syscall::STDIN, &mut orig_termios).is_ok() {
        let mut raw_termios = orig_termios.clone();
        set_raw_mode(&mut raw_termios);

        if set_termios(syscall::STDIN, TCSETS, &raw_termios).is_ok() {
            puts("Entered raw mode. Press q to exit.\r\n")?;
            puts("Opening file with mmap and displaying its contents.\r\n")?;

            match run_editor() {
                Ok(()) => {}
                Err(e) => {
                    puts("Error\r\n")?;
                    match e {
                        editor::EditorError::SysError(n) => write_number(n),
                        _ => todo!(),
                    }
                }
            }
            set_termios(syscall::STDIN, TCSETSW, &orig_termios)?;
            puts("\r\nExited raw mode\r\n")?;
        } else {
            puts("Failed to set raw mode\r\n")?;
        }
    } else {
        puts("Failed to get terminal attributes\r\n")?;
    }

    Ok(())
}
