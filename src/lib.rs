use std::{ffi::c_void, str};

extern "C" {
    fn cg3_run(
        grammar_data: *const u8,
        grammar_size: usize,
        input_data: *const u8,
        input_size: usize,
        output_size: *mut usize,
    ) -> *const c_void;

    fn cg3_free(stream: *const c_void);
    fn cg3_copy_output(stream: *const c_void, output: *mut u8, size: usize);
}

pub fn run(grammar: &[u8], input: &str) -> String {
    let mut output_size: usize = 0;

    let stream = unsafe {
        cg3_run(
            grammar.as_ptr(),
            grammar.len(),
            input.as_ptr(),
            input.len(),
            &mut output_size,
        )
    };

    let mut output = vec![0u8; output_size];
    unsafe {
        cg3_copy_output(stream, output.as_mut_ptr(), output_size);
    }

    unsafe {
        cg3_free(stream);
    }

    String::from_utf8(output).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let grammar = std::fs::read("./smj/grammarchecker-release.bin").unwrap();
        let input = "test input goes here";

        println!("WAT");
        println!("{}", run(&grammar, input));
    }
}
