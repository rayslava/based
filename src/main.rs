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

fn initialize_terminal() -> Result<(Winsize, Termios), usize> {
    let mut winsize = Winsize::new();
    get_winsize(syscall::STDOUT, &mut winsize)?;

    let mut orig_termios = Termios::new();
    get_termios(syscall::STDIN, &mut orig_termios)?;

    let mut raw_termios = orig_termios.clone();
    set_raw_mode(&mut raw_termios);
    set_termios(syscall::STDIN, TCSETS, &raw_termios)?;

    Ok((winsize, orig_termios))
}

fn restore_terminal(orig_termios: &Termios) -> Result<(), usize> {
    set_termios(syscall::STDIN, TCSETSW, orig_termios)?;
    Ok(())
}

fn handle_editor_error(e: &editor::EditorError) -> Result<(), usize> {
    puts("Error\r\n")?;
    match e {
        editor::EditorError::SysError(n) => {
            write_number(*n);
            Ok(())
        }
        _ => todo!(),
    }
}

fn run() -> Result<(), usize> {
    let (_, orig_termios) = initialize_terminal()?;

    let result = run_editor();

    if let Err(e) = result {
        handle_editor_error(&e)?;
    }

    restore_terminal(&orig_termios)
}
