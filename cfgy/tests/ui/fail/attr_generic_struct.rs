use cfgy::FromConfig;

#[derive(FromConfig)]
struct Settings<T> {
    value: T,
}

fn main() {}
