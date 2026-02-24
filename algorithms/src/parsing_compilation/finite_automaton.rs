pub struct FiniteAutomaton {
    pub transitions: Vec<Vec<Option<usize>>>,
    pub accepting: Vec<bool>,
}

impl FiniteAutomaton {
    pub fn accepts(&self, input: &[usize]) -> bool {
        let mut state = 0;
        for &symbol in input {
            match self.transitions[state][symbol] {
                Some(next) => state = next,
                None => return false,
            }
        }
        self.accepting[state]
    }
}
