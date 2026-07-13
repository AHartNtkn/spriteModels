use num_rational::Ratio;

pub(crate) fn abs_ratio(value: Ratio<i64>) -> Ratio<i64> {
    if value < Ratio::from_integer(0) {
        -value
    } else {
        value
    }
}
