use std::fmt::Debug;

pub trait DynAssertion: Send + Sync {
    fn display(&self);
    fn failed(&self) -> bool;
}
#[derive(Debug)]
pub struct AssertEQ<T> {
    pub left: T,
    pub right: T,
    pub result: bool,
}
#[derive(Debug)]
pub struct Assert {
    pub result: bool,
}

#[derive(Debug)]
pub enum Assertion<T> {
    Eq(AssertEQ<T>),
    Cond(Assert),
}

impl<T: Debug + Clone + Send + Sync> DynAssertion for Assertion<T> {
    fn display(&self) {
        println!("{:?}", self);
    }
    fn failed(&self) -> bool {
        match self {
            Assertion::Eq(a) => !a.result,
            Assertion::Cond(a) => !a.result,
        }
    }
}
pub struct AssertionLog {
    pub assertions: Vec<Box<dyn DynAssertion>>,
}

impl Default for AssertionLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AssertionLog {
    pub fn new() -> Self {
        AssertionLog {
            assertions: Vec::new(),
        }
    }

    pub fn insert(&mut self, assertion: Box<dyn DynAssertion>) {
        self.assertions.push(assertion);
    }

    pub fn len(&self) -> usize {
        self.assertions.len()
    }

    pub fn display_failures(&self, start: usize, end: usize) {
        for i in start..end {
            if self.assertions[i].failed() {
                self.assertions[i].display();
            }
        }
    }

    pub fn summarize(&self, start: usize, end: usize) -> (usize, usize) {
        let mut passed = 0;
        let mut failed = 0;
        for i in start..end {
            if self.assertions[i].failed() {
                failed += 1;
            } else {
                passed += 1;
            }
        }
        (passed, failed)
    }
}

pub fn eval_assert(cond: bool, file: &str, line: u32) -> bool {
    if !cond {
        println!("Assertion failed at {}:{}", file, line);
    }
    cond
}

pub fn eval_assert_eq<T: PartialEq + std::fmt::Debug>(a: T, b: T, file: &str, line: u32) -> bool {
    if a != b {
        println!("Assertion failed: {:?} != {:?} at {}:{}", a, b, file, line);
        false
    } else {
        true
    }
}

#[macro_export]
macro_rules! assert {
    ($cond:expr) => {
        let result = eval_assert(false, file!(), line!());
        let mut log = $crate::ASSERTION_LOG.get().unwrap();
        log.lock()
            .unwrap()
            .insert(Box::new(Assertion::<bool>::Cond(Assert { result })));
    };
}
#[macro_export]
macro_rules! assert_eq {
    ($a:expr, $b:expr) => {
        let result = eval_assert_eq($a, $b, file!(), line!());
        let log = $crate::ASSERTION_LOG.get().unwrap();
        log.lock()
            .unwrap()
            .insert(Box::new(Assertion::<i32>::Eq(AssertEQ {
                left: 1,
                right: 2,
                result,
            })));
    };
}
