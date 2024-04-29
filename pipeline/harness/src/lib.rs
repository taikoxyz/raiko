
// Define a struct to hold test details
struct Test {
    name: &'static str,
    test_fn: fn() -> (),
}

// Struct to manage and run tests
pub struct TestSuite {
    tests: Vec<Test>,
}

impl Default for TestSuite {
    fn default() -> Self {
        Self::new()
    }
}

impl TestSuite {
    // Create a new instance of a test suite
    pub fn new() -> Self {
        Self { tests: Vec::new() }
    }

    // Add a test to the suite
    pub fn add_test(&mut self, name: &'static str, test_fn: fn() -> ()) {
        self.tests.push(Test { name, test_fn });
    }

    // Run all tests in the suite
    pub fn run(&self) {
        let mut passed = 0;
        let mut failed = 0;
        for test in &self.tests {
            println!("Running test: {}", test.name);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test.test_fn));
            match result {
                Ok(_) => {
                    println!("    PASSED");
                    passed += 1;
                }
                Err(_) => {
                    println!("    FAILED");
                    failed += 1;
                }
            }
        }
        println!("\nTest results: {} passed; {} failed", passed, failed);
        if failed > 0 {
            std::process::exit(101);
        }
    }
}

#[macro_export]
macro_rules! zk_test {
    ($suite:expr, $name:ident) => {
        $suite.add_test(stringify!($name), $name);
    };
}

#[macro_export]
macro_rules! zk_suits {
    ($($test:ident),*) => {
        let mut test_suite = TestSuite::new();
        $(
            zk_test!(test_suite, $test);
        )*
        test_suite.run();
    };
}
