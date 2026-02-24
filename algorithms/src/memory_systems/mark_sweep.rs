pub struct Node {
    pub marked: bool,
    pub children: Vec<usize>,
}

pub fn mark(nodes: &mut [Node], root: usize) {
    fn dfs(nodes: &mut [Node], i: usize) {
        if nodes[i].marked {
            return;
        }
        nodes[i].marked = true;
        let children = nodes[i].children.clone();
        for c in children {
            dfs(nodes, c);
        }
    }
    dfs(nodes, root);
}

pub fn sweep(nodes: &mut Vec<Node>) {
    nodes.retain(|n| n.marked);
}
