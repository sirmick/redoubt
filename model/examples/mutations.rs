//! `cargo run -p redoubt-model --example mutations` prints every deliberate rule break's name, one
//! per line: the values the bench's `model-mutations` case runs `mutations_are_caught` over, one
//! job each, with `REDOUBT_MODEL_MUTATIONS` set to the name.

use redoubt_model::mutation::Mutation;

fn main() {
    for m in Mutation::ALL {
        println!("{m:?}");
    }
}
