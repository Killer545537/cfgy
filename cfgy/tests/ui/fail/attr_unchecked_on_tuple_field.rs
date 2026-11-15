use cfgy::FromConfig;

#[derive(FromConfig)]
struct Port(#[config(unchecked)] u16);

fn main() {}
