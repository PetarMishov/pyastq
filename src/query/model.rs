use std::collections::BTreeMap;

use regex::Regex;

pub type QueryVariables = BTreeMap<String, String>;

#[derive(Debug)]
pub struct Query {
    pub anchor: NodePattern,
    pub condition: Option<Expression>,
}

#[derive(Debug)]
pub enum Expression {
    Relation(Relationship, NodePattern),
    DescendantChain(Vec<NodePattern>),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
    Not(Box<Expression>),
}

#[derive(Clone, Copy, Debug)]
pub enum Relationship {
    Child,
    Descendant,
    Ancestor,
}

#[derive(Debug)]
pub struct NodePattern {
    pub kind: PatternKind,
    pub value: ValuePattern,
}

#[derive(Debug)]
pub enum PatternKind {
    Call,
    Class,
    Function,
    Import,
    Argument(ArgumentKey),
}

#[derive(Debug)]
pub enum ArgumentKey {
    Any,
    Position(usize),
    Keyword(String),
}

#[derive(Debug)]
pub enum ValuePattern {
    Any,
    Exact(String),
    Contains(String),
    StartsWith(String),
    EndsWith(String),
    Regex(Regex),
    Numeric(Comparison, f64),
    Capture(String),
}

#[derive(Clone, Copy, Debug)]
pub enum Comparison {
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}
