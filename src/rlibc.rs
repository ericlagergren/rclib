use super::libc;

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;

    use tempfile::tempfile;

    use super::*;

    #[test]
    fn test_write_basic() {
        let file = tempfile().unwrap();
        let fd = file.as_raw_fd();
        write(fd, xxx, yyy)
    }
}
