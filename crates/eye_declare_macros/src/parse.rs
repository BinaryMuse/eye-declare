use proc_macro2::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Pat, Result, Token, braced, parenthesized, token};

/// A node in the element tree.
pub enum Node {
    /// A component with optional props and optional children.
    Component {
        type_name: Ident,
        props: Vec<Prop>,
        children: Option<Vec<Node>>,
    },
    /// A string literal, rendered as Text.
    Text(LitStr),
    /// `#(if cond { ... })` or `#(if cond { ... } else { ... })`
    Conditional {
        condition: Expr,
        body: Vec<Node>,
        else_body: Option<Vec<Node>>,
    },
    /// `#(if let pat = expr { ... })`
    ConditionalLet {
        pattern: Pat,
        expr: Expr,
        body: Vec<Node>,
        else_body: Option<Vec<Node>>,
    },
    /// `#(for pat in iter { ... })`
    Loop {
        pattern: Pat,
        iter: Expr,
        body: Vec<Node>,
    },
    /// `#(expr)` — splice an Elements value inline.
    Splice(Expr),
}

/// A prop on a component: `name: value`.
pub struct Prop {
    pub name: Ident,
    pub value: Expr,
}

/// Parse a token stream into a list of nodes.
pub fn parse_nodes(input: TokenStream) -> Result<Vec<Node>> {
    syn::parse2::<NodeList>(input).map(|nl| nl.nodes)
}

struct NodeList {
    nodes: Vec<Node>,
}

impl Parse for NodeList {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut nodes = Vec::new();
        while !input.is_empty() {
            nodes.push(parse_node(input)?);
        }
        Ok(NodeList { nodes })
    }
}

fn parse_node(input: ParseStream) -> Result<Node> {
    // #(...) — control flow
    if input.peek(Token![#]) {
        return parse_control_flow(input);
    }

    // String literal — text node
    if input.peek(LitStr) {
        let lit: LitStr = input.parse()?;
        return Ok(Node::Text(lit));
    }

    // Ident — component
    if input.peek(Ident) {
        return parse_component(input);
    }

    Err(input.error("expected a component name, string literal, or #(...)"))
}

fn parse_component(input: ParseStream) -> Result<Node> {
    let type_name: Ident = input.parse()?;

    // Optional props in (...)
    let props = if input.peek(token::Paren) {
        let content;
        parenthesized!(content in input);
        parse_props(&content)?
    } else {
        Vec::new()
    };

    // Optional children in { ... }
    let children = if input.peek(token::Brace) {
        let content;
        braced!(content in input);
        let mut children = Vec::new();
        while !content.is_empty() {
            children.push(parse_node(&content)?);
        }
        Some(children)
    } else {
        None
    };

    Ok(Node::Component {
        type_name,
        props,
        children,
    })
}

fn parse_props(input: ParseStream) -> Result<Vec<Prop>> {
    let mut props = Vec::new();
    while !input.is_empty() {
        let name: Ident = input.parse()?;
        let _colon: Token![:] = input.parse()?;
        let value: Expr = input.parse()?;
        props.push(Prop { name, value });
        if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
        }
    }
    Ok(props)
}

fn parse_control_flow(input: ParseStream) -> Result<Node> {
    let _hash: Token![#] = input.parse()?;

    let content;
    parenthesized!(content in input);

    if content.peek(Token![if]) {
        parse_if(&content)
    } else if content.peek(Token![for]) {
        parse_for(&content)
    } else {
        // Any other expression — splice its Elements into the parent
        let expr: Expr = content.parse()?;
        Ok(Node::Splice(expr))
    }
}

fn parse_if(input: ParseStream) -> Result<Node> {
    let _if: Token![if] = input.parse()?;

    // Check for `if let`
    if input.peek(Token![let]) {
        return parse_if_let(input);
    }

    // Regular `if cond { ... }`
    // Use parse_without_eager_brace so syn doesn't consume { as part of the expr
    let condition: Expr = Expr::parse_without_eager_brace(input)?;

    let body_content;
    braced!(body_content in input);
    let mut body = Vec::new();
    while !body_content.is_empty() {
        body.push(parse_node(&body_content)?);
    }

    let else_body = if input.peek(Token![else]) {
        let _else: Token![else] = input.parse()?;
        let else_content;
        braced!(else_content in input);
        let mut else_nodes = Vec::new();
        while !else_content.is_empty() {
            else_nodes.push(parse_node(&else_content)?);
        }
        Some(else_nodes)
    } else {
        None
    };

    Ok(Node::Conditional {
        condition,
        body,
        else_body,
    })
}

fn parse_if_let(input: ParseStream) -> Result<Node> {
    let _let: Token![let] = input.parse()?;
    let pattern: Pat = Pat::parse_single(input)?;
    let _eq: Token![=] = input.parse()?;
    let expr: Expr = Expr::parse_without_eager_brace(input)?;

    let body_content;
    braced!(body_content in input);
    let mut body = Vec::new();
    while !body_content.is_empty() {
        body.push(parse_node(&body_content)?);
    }

    let else_body = if input.peek(Token![else]) {
        let _else: Token![else] = input.parse()?;
        let else_content;
        braced!(else_content in input);
        let mut else_nodes = Vec::new();
        while !else_content.is_empty() {
            else_nodes.push(parse_node(&else_content)?);
        }
        Some(else_nodes)
    } else {
        None
    };

    Ok(Node::ConditionalLet {
        pattern,
        expr,
        body,
        else_body,
    })
}

fn parse_for(input: ParseStream) -> Result<Node> {
    let _for: Token![for] = input.parse()?;
    let pattern: Pat = Pat::parse_multi(input)?;
    let _in: Token![in] = input.parse()?;
    let iter: Expr = Expr::parse_without_eager_brace(input)?;

    let body_content;
    braced!(body_content in input);
    let mut body = Vec::new();
    while !body_content.is_empty() {
        body.push(parse_node(&body_content)?);
    }

    Ok(Node::Loop {
        pattern,
        iter,
        body,
    })
}
