/// Takes the value out of the option, but only if the predicate evaluates to
/// `Some(U)`.
///
/// A `map`-like version of [`Option::take_if`], which queries the option for a
/// `Some` result rather than a bool, and forwards the produced value.
pub fn take_if_some<T, U>(
    opt: &mut Option<T>,
    predicate: impl FnOnce(&mut T) -> Option<U>,
) -> Option<(T, U)> {
    if let Some(out) = opt.as_mut().and_then(predicate) {
        // map() requires opt to be Some - take always succeeds
        Some((opt.take().unwrap(), out))
    } else {
        None
    }
}
