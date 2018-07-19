//! An example of using the `moniker` library to implement the simply typed
//! lambda calculus
//!
//! We use [bidirectional type checking](http://www.davidchristiansen.dk/tutorials/bidirectional.pdf)
//! to get some level of type inference.

extern crate im;
#[macro_use]
extern crate moniker;

use im::HashMap;
use moniker::{BoundTerm, Embed, FreeVar, PVar, Scope, TVar};
use std::rc::Rc;

/// Types
#[derive(Debug, Clone, BoundTerm)]
pub enum Type {
    /// Integers
    Int,
    /// Floating point numbers
    Float,
    /// Strings
    String,
    /// Function types
    Arrow(RcType, RcType),
}

/// Reference counted types
#[derive(Debug, Clone, BoundTerm)]
pub struct RcType {
    pub inner: Rc<Type>,
}

impl From<Type> for RcType {
    fn from(src: Type) -> RcType {
        RcType {
            inner: Rc::new(src),
        }
    }
}

/// Literal values
#[derive(Debug, Clone, BoundTerm)]
pub enum Literal {
    /// Integer literals
    Int(i32),
    /// Floating point literals
    Float(f32),
    /// String literals
    String(String),
}

/// Expressions
#[derive(Debug, Clone, BoundTerm)]
pub enum Expr {
    /// Annotated expressions
    Ann(RcExpr, RcType),
    /// Literals
    Literal(Literal),
    /// Variables
    Var(TVar<String>),
    /// Lambda expressions, with an optional type annotation for the parameter
    Lam(Scope<(PVar<String>, Embed<Option<RcType>>), RcExpr>),
    /// Function application
    App(RcExpr, RcExpr),
}

/// Reference counted expressions
#[derive(Debug, Clone, BoundTerm)]
pub struct RcExpr {
    pub inner: Rc<Expr>,
}

impl From<Expr> for RcExpr {
    fn from(src: Expr) -> RcExpr {
        RcExpr {
            inner: Rc::new(src),
        }
    }
}

impl RcExpr {
    // FIXME: auto-derive this somehow!
    fn subst<N>(&self, name: &N, replacement: &RcExpr) -> RcExpr
    where
        TVar<String>: PartialEq<N>,
    {
        match *self.inner {
            Expr::Ann(ref expr, ref ty) => {
                RcExpr::from(Expr::Ann(expr.subst(name, replacement), ty.clone()))
            },
            Expr::Var(ref n) if n == name => replacement.clone(),
            Expr::Var(_) | Expr::Literal(_) => self.clone(),
            Expr::Lam(ref scope) => RcExpr::from(Expr::Lam(Scope {
                unsafe_pattern: scope.unsafe_pattern.clone(),
                unsafe_body: scope.unsafe_body.subst(name, replacement),
            })),
            Expr::App(ref fun, ref arg) => RcExpr::from(Expr::App(
                fun.subst(name, replacement),
                arg.subst(name, replacement),
            )),
        }
    }
}

/// Evaluate an expression into its normal form
pub fn eval(expr: &RcExpr) -> RcExpr {
    match *expr.inner {
        Expr::Ann(ref expr, _) => eval(expr),
        Expr::Literal(_) | Expr::Var(_) | Expr::Lam(_) => expr.clone(),
        Expr::App(ref fun, ref arg) => match *eval(fun).inner {
            Expr::Lam(ref scope) => {
                let ((name, _), body) = scope.clone().unbind();
                eval(&body.subst(&name, &eval(arg)))
            },
            _ => expr.clone(),
        },
    }
}

/// A context containing a series of type annotations
type Context = HashMap<FreeVar<String>, RcType>;

/// Check that a (potentially ambiguous) expression conforms to a given type
pub fn check(context: &Context, expr: &RcExpr, expected_ty: &RcType) -> Result<(), String> {
    match (&*expr.inner, &*expected_ty.inner) {
        (&Expr::Lam(ref scope), &Type::Arrow(ref param_ty, ref ret_ty)) => {
            if let ((name, Embed(None)), body) = scope.clone().unbind() {
                // FIXME: Ick!
                let name = name
                    .try_into_free_var()
                    .expect("encountered a bound variable");
                check(&context.insert(name, param_ty.clone()), &body, ret_ty)?;
                return Ok(());
            }
        },
        (_, _) => {},
    }

    let inferred_ty = infer(&context, expr)?;

    if RcType::term_eq(&inferred_ty, expected_ty) {
        Ok(())
    } else {
        Err(format!(
            "type mismatch - found `{:?}` but expected `{:?}`",
            inferred_ty, expected_ty
        ))
    }
}

/// Synthesize the types of unambiguous expressions
pub fn infer(context: &Context, expr: &RcExpr) -> Result<RcType, String> {
    match *expr.inner {
        Expr::Ann(ref expr, ref ty) => {
            check(context, expr, ty)?;
            Ok(ty.clone())
        },
        Expr::Literal(Literal::Int(_)) => Ok(RcType::from(Type::Int)),
        Expr::Literal(Literal::Float(_)) => Ok(RcType::from(Type::Float)),
        Expr::Literal(Literal::String(_)) => Ok(RcType::from(Type::String)),
        Expr::Var(ref var) => match context.get(
            // FIXME: Ick!
            &var.clone()
                .try_into_free_var()
                .expect("encountered a bound variable"),
        ) {
            Some(term) => Ok((*term).clone()),
            None => Err(format!("`{:?}` not found in `{:?}`", var, context)),
        },
        Expr::Lam(ref scope) => match scope.clone().unbind() {
            ((name, Embed(Some(ann))), body) => {
                // FIXME: Ick!
                let name = name
                    .try_into_free_var()
                    .expect("encountered a bound variable");
                let body_ty = infer(&context.insert(name, ann.clone()), &body)?;
                Ok(RcType::from(Type::Arrow(ann, body_ty)))
            },
            ((name, Embed(None)), _) => {
                Err(format!("type annotation needed for argument `{:?}`", name))
            },
        },
        Expr::App(ref fun, ref arg) => match *infer(context, fun)?.inner {
            Type::Arrow(ref param_ty, ref ret_ty) => {
                let arg_ty = infer(context, arg)?;
                if RcType::term_eq(param_ty, &arg_ty) {
                    Ok(ret_ty.clone())
                } else {
                    Err(format!(
                        "argument type mismatch - found `{:?}` but expected `{:?}`",
                        arg_ty, param_ty,
                    ))
                }
            },
            _ => Err(format!("`{:?}` is not a function", fun)),
        },
    }
}

#[test]
fn test_infer() {
    // expr = (\x : Int -> x)
    let expr = RcExpr::from(Expr::Lam(Scope::new(
        (PVar::user("x"), Embed(Some(RcType::from(Type::Int)))),
        RcExpr::from(Expr::Var(TVar::user("x"))),
    )));

    assert_term_eq!(
        infer(&Context::new(), &expr).unwrap(),
        RcType::from(Type::Arrow(
            RcType::from(Type::Int),
            RcType::from(Type::Int)
        )),
    );
}

// TODO: Use property testing for this!
// http://janmidtgaard.dk/papers/Midtgaard-al%3AICFP17-full.pdf

fn main() {}
