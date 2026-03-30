pub mod detect;
pub mod flutter;
pub mod gotest;
pub mod gotest_json;
pub mod jest;
pub mod junit;
pub mod libtest;
pub mod libtest_json;
pub mod model;
pub mod pytest;
pub mod tap;

pub use detect::detect_format;
pub use model::{Format, TestResult, TestRun, TestStatus};

/// Parse complete test output into a TestRun.
pub fn parse_output(input: &str, format: Format) -> TestRun {
    match format {
        Format::Libtest => libtest::parse_libtest(input),
        Format::LibtestJson => libtest_json::parse_libtest_json(input),
        Format::Pytest => pytest::parse_pytest(input),
        Format::Junit => junit::parse_junit(input),
        Format::Jest => jest::parse_jest(input),
        Format::GoTest => gotest::parse_gotest(input),
        Format::GoTestJson => gotest_json::parse_gotest_json(input),
        Format::Tap => tap::parse_tap(input),
        Format::Flutter => flutter::parse_flutter(input),
        Format::Unknown => TestRun::from_results(vec![], None, format),
    }
}
