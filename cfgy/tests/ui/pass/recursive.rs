use cfgy::FromConfig;

// A field naming its own struct gets `Shape::Any`, which cuts the const cycle; the type is still loaded in full at
// runtime. Also shows a user type named `Node` does not collide with the check's document type.
#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "fixtures/pass/recursive.toml")]
struct Node {
    name: String,
    children: Vec<Node>,
}

fn leaf(name: &str) -> Node {
    Node { name: name.to_owned(), children: Vec::new() }
}

fn main() {
    let tree = Node::load_str(include_str!("recursive.toml"), &cfgy::Toml).unwrap();
    let expected = Node {
        name: "root".to_owned(),
        children: vec![Node { name: "a".to_owned(), children: vec![leaf("a1")] }, leaf("b")],
    };
    assert_eq!(tree, expected);
}
