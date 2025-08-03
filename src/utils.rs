use std::ops::ControlFlow;

/// Repeats a closure similar to a `for` loop, advancing the state using another
/// closure based on feedback from the loop.
///
/// Both the called closure and the iterator closure can decide to stop the loop
/// by returning [`ControlFlow::Break`]. The iteration index is also provided
/// automatically. Both closures are [`FnMut`], so they can keep an internal
/// mutable state, too.
///
/// These closures are run in sequence, effectively no different from a loop
/// with iteration statements at the end, but this wrapper helps separate the
/// action and iteration. The main closure takes `T` as input, producing
/// [`ControlFlow<B, U>`][`ControlFlow`] as feedback ([`ControlFlow::Continue`]
/// to continue, [`ControlFlow::Break`] to terminate). The `after_each` iterator
/// then receives `U` as input, producing [`ControlFlow<B, T>`][`ControlFlow`]
/// again for the next iteration (or, again, returning [`ControlFlow::Break`] to
/// terminate).
pub fn loop_with_feedback<T, U, B, F, C>(initial: T, mut after_each: F, mut closure: C) -> B
where
    F: FnMut(usize, U) -> ControlFlow<B, T>,
    C: FnMut(usize, T) -> ControlFlow<B, U>,
{
    let mut input = initial;
    let mut iteration = 0;
    loop {
        let output = match closure(iteration, input) {
            ControlFlow::Continue(output) => output,
            ControlFlow::Break(result) => return result,
        };
        input = match after_each(iteration, output) {
            ControlFlow::Continue(input) => input,
            ControlFlow::Break(result) => return result,
        };

        iteration += 1;
    }
}
