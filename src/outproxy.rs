use std::io::{stdout, Write};

pub struct OutProxy {
    pub n_lines: usize,
}

impl Write for OutProxy {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.n_lines += count_newlines(buf);
        stdout().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        stdout().flush()
    }
}

impl OutProxy {
    pub fn new() -> Self {
        OutProxy { n_lines: 0 }
    }
}

#[cfg(target_os = "windows")]
fn count_newlines(buf: &[u8]) -> usize {
    let mut count = 0;
    for i in 0..(buf.len() - 1) {
        if buf[i] as char == '\r' && buf[i + 1] as char == '\n' {
            count += 1;
        }
    }
    count
}

#[cfg(not(target_os = "windows"))]
fn count_newlines(buf: &[u8]) -> usize {
    buf.iter().filter(|x| **x as char == '\n').count()
}
