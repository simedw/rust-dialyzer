///! This pass combines two rewrite patterns:
///!
///! First, it lowers `receive` to more primitive operations, see the example below.
///! Second, it rewrites alias patterns to nested cases.
///!
///! ## Example
///!
///! Given this source function:
///!
///! ```erlang,ignore
///! foo(Timeout) ->
///!     receive
///!         {tag,Msg} -> Msg
///!     after
///!         Timeout ->
///!             no_message
///!     end.
///! ```
///!
///! The Core IR after this pass will look like the following:
///!
///! ```ignore
///! 'foo'/1 =
///!     fun (Timeout) ->
///!         ( letrec
///!               'recv$^0'/0 =
///!                   fun () ->
///!                       let <PeekSucceeded,Message> =
///!                           primop 'recv_peek_message'()
///!                       in  case PeekSucceeded of
///!                             <'true'> when 'true' ->
///!                                 case Message of
///!                                   <{'tag',Msg}> when 'true' ->
///!                                       do  primop 'remove_message'()
///!                                           Msg
///!                                   ( <Other> when 'true' ->
///!                                         do  primop 'recv_next'()
///!                                             apply 'recv$^0'/0()
///!                                     -| ['compiler_generated'] )
///!                                 end
///!                             <'false'> when 'true' ->
///!                                 let <TimedOut> =
///!                                     primop 'recv_wait_timeout'(Timeout)
///!                                 in  case TimedOut of
///!                                       <'true'> when 'true' ->
///!                                           'no_message'
///!                                       <'false'> when 'true' ->
///!                                           apply 'recv$^0'/0()
///!                                     end
///!                           end
///!           in  apply 'recv$^0'/0()
///!           -| ['letrec_goto'] )
///! ```
///!
///! As you can see, the receive no longer exists, having been rewritten into
///! a `letrec` expression with calls to various BIFs that implement the receive
///! primitives.
use std::cell::UnsafeCell;
use std::rc::Rc;

use rpds::{rbt_set, RedBlackTreeSet};

use firefly_binary::BinaryEntrySpecifier;
use firefly_diagnostics::*;
use firefly_intern::{symbols, Ident, Symbol};
use firefly_pass::Pass;
use firefly_syntax_base::*;

use crate::{passes::FunctionContext, *};

pub struct RewriteReceivePrimitives {
    context: Rc<UnsafeCell<FunctionContext>>,
    dialyzer: bool,
}
impl RewriteReceivePrimitives {
    pub fn new(context: Rc<UnsafeCell<FunctionContext>>) -> Self {
        Self { context, dialyzer: false }
    }

    pub fn for_dialyzer(mut self) -> Self {
        self.dialyzer = true;
        self
    }

    fn timeout_is_ignored(&self, timeout: &Expr) -> bool {
        self.dialyzer && matches!(timeout, Expr::Literal(Literal { value: Lit::Atom(atom), .. }) if *atom == symbols::Infinity)
    }
    fn receive_switch(
        span: SourceSpan,
        annotations: Annotations,
        guard: Box<Expr>,
        then_body: Box<Expr>,
        else_body: Box<Expr>,
        ignored_branch: Option<bool>,
    ) -> Expr {
        if let Some(ignored_branch) = ignored_branch {
            let clauses = [(true, symbols::True, then_body), (false, symbols::False, else_body)]
                .into_iter()
                .map(|(branch, atom, body)| {
                    let mut clause = Clause::new(span, vec![Expr::Literal(Literal::atom(span, atom))], *body);
                    if branch == ignored_branch {
                        clause.annotations.insert_mut(Symbol::intern("dialyzer_ignore"), Annotation::Unit);
                    }
                    clause
                }).collect();
            Expr::Case(Case { span, annotations, arg: guard, clauses })
        } else {
            Expr::If(If { span, annotations, guard, then_body, else_body })
        }
    }

    #[inline(always)]
    fn context_mut(&self) -> &mut FunctionContext {
        unsafe { &mut *self.context.get() }
    }
}
impl Pass for RewriteReceivePrimitives {
    type Input<'a> = Fun;
    type Output<'a> = Fun;

    fn run<'a>(&mut self, mut fun: Self::Input<'a>) -> anyhow::Result<Self::Output<'a>> {
        self.lexpr(fun.body.as_mut())?;
        Ok(fun)
    }
}

type WrapperFn = dyn FnOnce(Expr) -> Box<Expr>;

struct Split {
    args: Vec<Expr>,
    wrapper: Option<Box<WrapperFn>>,
    pattern: Box<Expr>,
    nested: Option<Box<Split>>,
}
impl Split {
    fn new(args: Vec<Expr>, pattern: Expr, nested: Option<Box<Self>>) -> Box<Self> {
        Box::new(Self {
            args,
            wrapper: None,
            pattern: Box::new(pattern),
            nested,
        })
    }

    fn new_wrapped(
        args: Vec<Expr>,
        pattern: Expr,
        wrapper: Option<Box<WrapperFn>>,
        nested: Option<Box<Self>>,
    ) -> Box<Self> {
        Box::new(Self {
            args,
            wrapper,
            pattern: Box::new(pattern),
            nested,
        })
    }
}

impl RewriteReceivePrimitives {
    fn lexpr(&mut self, expr: &mut Expr) -> anyhow::Result<()> {
        match expr {
            // Skip nodes that do not have case or receive within them,
            // so we can speed up lowering by not traversing them.
            Expr::Binary(_)
            | Expr::Call(_)
            | Expr::Cons(_)
            | Expr::Literal(_)
            | Expr::Map(_)
            | Expr::PrimOp(_)
            | Expr::Tuple(_)
            | Expr::Var(_) => Ok(()),
            Expr::Case(ref mut case) => {
                // Visit the child nodes of the tree first
                self.lexpr(case.arg.as_mut())?;
                self.lexpr_clauses(case.clauses.as_mut_slice())?;
                // Then, split patterns that bind and use the same variable
                match self.split_case(case)? {
                    None => Ok(()),
                    Some(e) => {
                        *expr = e;
                        Ok(())
                    }
                }
            }
            Expr::Receive(ref mut recv) if recv.clauses.is_empty() => {
                // Visit the child nodes of the tree first
                self.lexpr(recv.timeout.as_mut())?;
                self.lexpr(recv.action.as_mut())?;

                // Lower a receive with only an after to its primitive operations
                let recv_span = recv.span();
                let f = Expr::Literal(Literal::atom(recv_span, symbols::False));

                let (timeout, outer) = if recv.timeout.is_safe() {
                    (recv.timeout.as_ref().clone(), f.clone())
                } else {
                    let span = recv.timeout.span();
                    let timeout = self.context_mut().next_var(Some(span));
                    let outer = Expr::Let(Let {
                        span,
                        annotations: Annotations::default(),
                        vars: vec![timeout.clone()],
                        arg: recv.timeout.clone(),
                        body: Box::new(f.clone()),
                    });
                    (Expr::Var(timeout), outer)
                };

                let loop_name = self.context_mut().new_fun_name(Some("recv"));
                let loop_fun = Var::new_with_arity(Ident::with_empty_span(loop_name), 0);
                let apply_loop = Box::new(Expr::Apply(Apply {
                    span: recv_span,
                    annotations: Annotations::default(),
                    callee: Box::new(Expr::Var(loop_fun.clone())),
                    args: vec![],
                }));

                let span = timeout.span();
                let timeout_bool = self.context_mut().next_var(Some(span));
                let body = Box::new(Self::receive_switch(
                    span,
                    Annotations::from([symbols::ReceiveTimeout]),
                    Box::new(Expr::Var(timeout_bool.clone())),
                    recv.action.clone(),
                    apply_loop.clone(),
                    self.timeout_is_ignored(&timeout).then_some(true),
                ));
                let timeout_let = Box::new(Expr::Let(Let {
                    span,
                    annotations: Annotations::default(),
                    vars: vec![timeout_bool],
                    arg: Box::new(Expr::PrimOp(PrimOp::new(
                        span,
                        symbols::RecvWaitTimeout,
                        vec![timeout],
                    ))),
                    body,
                }));
                let fun = Expr::Fun(Fun {
                    span: recv_span,
                    annotations: Annotations::default(),
                    name: loop_name,
                    vars: vec![],
                    body: timeout_let,
                });

                let lr = Expr::LetRec(LetRec {
                    span: recv_span,
                    annotations: Annotations::from([symbols::LetrecGoto, symbols::NoInline]),
                    defs: vec![(loop_fun, fun)],
                    body: apply_loop,
                });

                // If the 'after' expression is unsafe, we evaluate it in an outer 'let'
                let outer = match outer {
                    Expr::Let(mut outer_let) => {
                        outer_let.body = Box::new(lr);
                        Expr::Let(outer_let)
                    }
                    _ => lr,
                };
                *expr = outer;
                Ok(())
            }
            Expr::Receive(ref mut recv) => {
                // Visit the child nodes of the tree first
                self.lexpr_clauses(recv.clauses.as_mut_slice())?;
                self.lexpr(recv.timeout.as_mut())?;
                self.lexpr(recv.action.as_mut())?;

                // Lower receive to its primitive operations
                let recv_span = recv.span();
                // Lower a receive with only an after to its primitive operations
                let f = Expr::Literal(Literal::atom(recv_span, symbols::False));

                let (timeout, outer) = if recv.timeout.is_safe() {
                    (recv.timeout.as_ref().clone(), f.clone())
                } else {
                    let span = recv.timeout.span();
                    let timeout = self.context_mut().next_var(Some(span));
                    let outer = Expr::Let(Let {
                        span,
                        annotations: Annotations::default(),
                        vars: vec![timeout.clone()],
                        arg: recv.timeout.clone(),
                        body: Box::new(f.clone()),
                    });
                    (Expr::Var(timeout), outer)
                };

                let loop_name = self.context_mut().new_fun_name(Some("recv"));
                let loop_fun = Var::new_with_arity(Ident::with_empty_span(loop_name), 0);
                let apply_loop = Box::new(Expr::Apply(Apply {
                    span: recv_span,
                    annotations: Annotations::default(),
                    callee: Box::new(Expr::Var(loop_fun.clone())),
                    args: vec![],
                }));

                let mut clauses = Vec::with_capacity(recv.clauses.len());
                clauses.append(&mut recv.clauses);
                let mut clauses = self.rewrite_clauses(clauses);

                let recv_next = Box::new(Expr::Seq(Seq {
                    span: recv_span,
                    annotations: Annotations::default(),
                    arg: Box::new(Expr::PrimOp(PrimOp::new(
                        recv_span,
                        symbols::RecvNext,
                        vec![],
                    ))),
                    body: apply_loop.clone(),
                }));
                let recv_next_clause = Clause {
                    span: recv_span,
                    annotations: Annotations::default_compiler_generated(),
                    patterns: vec![Expr::Var(Var::new(Ident::new(symbols::Other, recv_span)))],
                    guard: None,
                    body: recv_next,
                };
                clauses.push(recv_next_clause);

                let msg = self.context_mut().next_var(Some(recv_span));
                let mut msg_case = Case {
                    span: recv_span,
                    annotations: recv.annotations.clone(),
                    arg: Box::new(Expr::Var(msg.clone())),
                    clauses,
                };
                let msg_case = match self.split_case(&mut msg_case)? {
                    None => Box::new(Expr::Case(msg_case)),
                    Some(case) => Box::new(case),
                };

                let span = recv.timeout.span();
                let timeout_bool = self.context_mut().next_var(Some(span));
                let timeout_body = Box::new(Expr::If(If {
                    span,
                    annotations: Annotations::default(),
                    guard: Box::new(Expr::Var(timeout_bool.clone())),
                    then_body: recv.action.clone(),
                    else_body: apply_loop.clone(),
                }));
                let timeout_let = Box::new(Expr::Let(Let {
                    span,
                    annotations: Annotations::default(),
                    vars: vec![timeout_bool],
                    arg: Box::new(Expr::PrimOp(PrimOp::new(
                        span,
                        symbols::RecvWaitTimeout,
                        vec![timeout],
                    ))),
                    body: timeout_body,
                }));

                let peek_succeeded = self.context_mut().next_var(Some(span));
                let peek_body = Box::new(Self::receive_switch(
                    span,
                    Annotations::default(),
                    Box::new(Expr::Var(peek_succeeded.clone())),
                    msg_case,
                    timeout_let,
                    self.timeout_is_ignored(&recv.timeout).then_some(false),
                ));
                let peek_let = Box::new(Expr::Let(Let {
                    span,
                    annotations: Annotations::default(),
                    vars: vec![peek_succeeded, msg],
                    arg: Box::new(Expr::PrimOp(PrimOp::new(
                        span,
                        symbols::RecvPeekMessage,
                        vec![],
                    ))),
                    body: peek_body,
                }));
                let fun = Expr::Fun(Fun {
                    span: recv_span,
                    annotations: Annotations::default(),
                    name: loop_name,
                    vars: vec![],
                    body: peek_let,
                });
                let lr = Expr::LetRec(LetRec {
                    span: recv_span,
                    annotations: Annotations::from([symbols::LetrecGoto, symbols::NoInline]),
                    defs: vec![(loop_fun, fun)],
                    body: apply_loop,
                });

                // If the 'after' expression is unsafe, evaluate it in an outer 'let'
                let outer = match outer {
                    Expr::Let(mut outer_let) => {
                        outer_let.body = Box::new(lr);
                        Expr::Let(outer_let)
                    }
                    _ => lr,
                };
                *expr = outer;
                Ok(())
            }
            Expr::Alias(ref mut alias) => self.lexpr(alias.pattern.as_mut()),
            Expr::Apply(ref mut apply) => {
                self.lexpr(apply.callee.as_mut())?;
                self.lexprs(apply.args.as_mut_slice())
            }
            Expr::Catch(ref mut catch) => self.lexpr(catch.body.as_mut()),
            Expr::Fun(ref mut fun) => self.lexpr(fun.body.as_mut()),
            Expr::If(ref mut ife) => {
                self.lexpr(ife.then_body.as_mut())?;
                self.lexpr(ife.else_body.as_mut())
            }
            Expr::Let(ref mut expr) => {
                self.lexpr(expr.arg.as_mut())?;
                self.lexpr(expr.body.as_mut())
            }
            Expr::LetRec(ref mut expr) => {
                for (_, ref mut def) in expr.defs.iter_mut() {
                    self.lexpr(def)?;
                }
                self.lexpr(expr.body.as_mut())
            }
            Expr::Seq(ref mut expr) => {
                self.lexpr(expr.arg.as_mut())?;
                self.lexpr(expr.body.as_mut())
            }
            Expr::Try(ref mut expr) => {
                self.lexpr(expr.arg.as_mut())?;
                self.lexpr(expr.body.as_mut())?;
                self.lexpr(expr.handler.as_mut())
            }
            Expr::Values(ref mut expr) => self.lexprs(expr.values.as_mut_slice()),
        }
    }

    fn lexprs(&mut self, exprs: &mut [Expr]) -> anyhow::Result<()> {
        for expr in exprs.iter_mut() {
            self.lexpr(expr)?;
        }
        Ok(())
    }

    fn lexpr_clauses(&mut self, clauses: &mut [Clause]) -> anyhow::Result<()> {
        for clause in clauses.iter_mut() {
            self.lexprs(clause.patterns.as_mut_slice())?;
            if let Some(guard) = clause.guard.as_deref_mut() {
                self.lexpr(guard)?;
            }
            self.lexpr(clause.body.as_mut())?;
        }
        Ok(())
    }

    fn rewrite_clauses(&mut self, mut clauses: Vec<Clause>) -> Vec<Clause> {
        clauses
            .drain(..)
            .map(|clause| {
                let span = clause.span;
                let body = Box::new(Expr::Seq(Seq {
                    span,
                    annotations: Annotations::default(),
                    arg: Box::new(Expr::PrimOp(PrimOp::new(
                        span,
                        symbols::RemoveMessage,
                        vec![],
                    ))),
                    body: clause.body,
                }));
                Clause {
                    span,
                    annotations: clause.annotations,
                    patterns: clause.patterns,
                    guard: clause.guard,
                    body,
                }
            })
            .collect()
    }

    /// Split patterns such as <<Size:32,Tail:Size>> that bind
    /// and use a variable in the same pattern. Rewrite to a
    /// nested case in a letrec.
    fn split_case(&mut self, case: &mut Case) -> anyhow::Result<Option<Expr>> {
        let span = case.span;
        let args = match case.arg.as_ref() {
            Expr::Values(vs) => vs.values.iter().cloned().collect(),
            arg => vec![arg.clone()],
        };
        let varargs = self.split_var_args(args.as_slice());
        match self.split_clauses(
            case.clauses.as_slice(),
            varargs.as_slice(),
            case.annotations.clone(),
        ) {
            None => Ok(None),
            Some((pre_case, after_cs)) => {
                let after_case = Box::new(Expr::Case(Case {
                    span,
                    annotations: case.annotations.clone(),
                    arg: Box::new(Values::new(span, varargs.clone())),
                    clauses: after_cs,
                }));
                let after_fun = Fun {
                    span,
                    name: symbols::Empty,
                    annotations: Annotations::default(),
                    vars: vec![],
                    body: after_case,
                };
                let lrec = self.split_case_letrec(after_fun, Box::new(Expr::Case(pre_case)))?;
                Ok(Some(split_letify(varargs, args, lrec)))
            }
        }
    }

    fn split_var_args(&mut self, args: &[Expr]) -> Vec<Expr> {
        args.iter()
            .map(|arg| match arg {
                arg @ (Expr::Var(_) | Expr::Literal(_)) => arg.clone(),
                _ => Expr::Var(self.context_mut().next_var(None)),
            })
            .collect()
    }

    fn split_case_letrec(&mut self, mut fun: Fun, body: Box<Expr>) -> anyhow::Result<Box<Expr>> {
        let span = fun.span();
        fun.mark_compiler_generated();
        let mut annotations = Annotations::default();
        annotations.set(symbols::LetrecGoto);
        annotations.set(symbols::Inline);
        let fname = self.context_mut().goto_func();
        self.context_mut().inc_goto_func();
        let mut expr = Box::new(Expr::LetRec(LetRec {
            span,
            annotations,
            defs: vec![(fname.clone(), Expr::Fun(fun))],
            body,
        }));
        self.lexpr(expr.as_mut())?;
        Ok(expr)
    }

    fn split_clauses(
        &mut self,
        mut clauses: &[Clause],
        args: &[Expr],
        anno: Annotations,
    ) -> Option<(Case, Vec<Clause>)> {
        match clauses.split_off_first() {
            None => None,
            Some(clause) => match self.split_clauses(clauses, args, anno.clone()) {
                None => match self.split_clause(clause) {
                    None => None,
                    Some((ps, nested)) => {
                        let case = self.split_reconstruct(args, ps, nested, clause, anno);
                        Some((case, clauses.iter().cloned().collect()))
                    }
                },
                Some((mut case, cs)) => {
                    case.clauses.insert(0, clause.clone());
                    Some((case, cs))
                }
            },
        }
    }

    fn split_clause(&mut self, clause: &Clause) -> Option<(Vec<Expr>, Option<Box<Split>>)> {
        self.split_pats(clause.patterns.as_slice())
    }

    fn split_reconstruct(
        &mut self,
        args: &[Expr],
        ps: Vec<Expr>,
        split: Option<Box<Split>>,
        clause: &Clause,
        annotations: Annotations,
    ) -> Case {
        let span = clause.span;
        match split {
            None => {
                let fc = self.split_fc_clause(ps.as_slice(), span, annotations.clone());
                let c = Clause {
                    span,
                    annotations: clause.annotations.clone(),
                    patterns: ps,
                    guard: clause.guard.clone(),
                    body: clause.body.clone(),
                };
                Case {
                    span,
                    annotations,
                    arg: Box::new(Values::new(span, args.iter().cloned().collect())),
                    clauses: vec![c, fc],
                }
            }
            Some(split) => {
                let inner_case = self.split_reconstruct(
                    split.args.as_slice(),
                    vec![*split.pattern],
                    split.nested,
                    clause,
                    annotations.clone(),
                );
                let fc = self.split_fc_clause(args, inner_case.span(), clause.annotations.clone());
                let wrapped = match split.wrapper {
                    None => Box::new(Expr::Case(inner_case)),
                    Some(wrap) => wrap(Expr::Case(inner_case)),
                };
                let c = Clause {
                    span,
                    annotations: clause.annotations.clone(),
                    patterns: ps,
                    guard: None,
                    body: wrapped,
                };
                Case {
                    span,
                    annotations,
                    arg: Box::new(Values::new(span, args.to_vec())),
                    clauses: vec![c, fc],
                }
            }
        }
    }

    fn split_fc_clause(
        &mut self,
        args: &[Expr],
        span: SourceSpan,
        mut annotations: Annotations,
    ) -> Clause {
        annotations.set(symbols::CompilerGenerated);
        let arity = args.len();
        let vars = self
            .context_mut()
            .next_n_vars(arity, None)
            .drain(..)
            .map(Expr::Var)
            .collect();
        let op = self.context_mut().goto_func();
        let apply = Expr::Apply(Apply {
            span,
            annotations: annotations.clone(),
            callee: Box::new(Expr::Var(op)),
            args: vec![],
        });
        Clause {
            span,
            annotations,
            patterns: vars,
            guard: None,
            body: Box::new(apply),
        }
    }

    fn split_pats(&mut self, mut patterns: &[Expr]) -> Option<(Vec<Expr>, Option<Box<Split>>)> {
        match patterns.split_off_first() {
            None => None,
            Some(pattern) => match self.split_pats(patterns) {
                None => match self.split_pat(pattern) {
                    None => None,
                    Some((p, split)) => {
                        let ps = core::iter::once(p)
                            .chain(patterns.iter().cloned())
                            .collect();
                        Some((ps, split))
                    }
                },
                Some((mut ps, split)) => {
                    ps.insert(0, pattern.clone());
                    Some((ps, split))
                }
            },
        }
    }

    fn split_pat(&mut self, pattern: &Expr) -> Option<(Expr, Option<Box<Split>>)> {
        match pattern {
            Expr::Binary(Binary {
                span,
                annotations,
                segments,
            }) => {
                let span = *span;
                let vars = rbt_set![];
                match self.split_bin_segments(segments.as_slice(), vars, vec![]) {
                    None => None,
                    Some((tailvar, wrap, bef, aft)) => {
                        let befbin = Expr::Binary(Binary {
                            span,
                            annotations: annotations.clone(),
                            segments: bef,
                        });
                        let bin = Expr::Binary(Binary {
                            span,
                            annotations: annotations.clone(),
                            segments: aft,
                        });
                        Some((
                            befbin,
                            Some(Split::new_wrapped(
                                vec![Expr::Var(tailvar)],
                                bin,
                                wrap,
                                None,
                            )),
                        ))
                    }
                }
            }
            Expr::Map(ref map) => self.split_map_pat(map.pairs.as_slice(), map.clone(), vec![]),
            Expr::Var(_) => None,
            Expr::Alias(ref alias) => match self.split_pat(alias.pattern.as_ref()) {
                None => None,
                Some((ps, split)) => {
                    let var = self.context_mut().next_var(None);
                    let alias = Expr::Alias(Alias {
                        span: alias.span,
                        annotations: alias.annotations.clone(),
                        var: alias.var.clone(),
                        pattern: Box::new(Expr::Var(var.clone())),
                    });
                    Some((alias, Some(Split::new(vec![Expr::Var(var)], ps, split))))
                }
            },
            Expr::Cons(Cons {
                span,
                annotations,
                head,
                tail,
            }) => {
                let span = *span;
                let elements = vec![head.as_ref().clone(), tail.as_ref().clone()];
                let (mut elements, split) = self.split_data(elements.as_slice(), vec![])?;
                assert_eq!(elements.len(), 2);
                let tail = elements.pop().unwrap();
                let head = elements.pop().unwrap();
                let cons = Expr::Cons(Cons {
                    span,
                    annotations: annotations.clone(),
                    head: Box::new(head),
                    tail: Box::new(tail),
                });
                Some((cons, split))
            }
            Expr::Tuple(Tuple {
                span,
                annotations,
                elements,
            }) => {
                let span = *span;
                let (elements, split) = self.split_data(elements.as_slice(), vec![])?;
                let tuple = Expr::Tuple(Tuple {
                    span,
                    annotations: annotations.clone(),
                    elements,
                });
                Some((tuple, split))
            }
            Expr::Literal(_) => None,
            other => panic!("unexpected pattern expression in split_pat: {:?}", &other),
        }
    }

    fn split_map_pat(
        &mut self,
        mut pairs: &[MapPair],
        map: Map,
        mut acc: Vec<MapPair>,
    ) -> Option<(Expr, Option<Box<Split>>)> {
        match pairs.split_off_first() {
            None => None,
            Some(MapPair { op, key, value }) => match key.as_ref() {
                Expr::Var(_) | Expr::Literal(_) => match self.split_pat(value.as_ref()) {
                    None => {
                        acc.push(MapPair {
                            op: *op,
                            key: key.clone(),
                            value: value.clone(),
                        });
                        self.split_map_pat(pairs, map, acc)
                    }
                    Some((p, split)) => {
                        let var = self.context_mut().next_var(None);
                        acc.push(MapPair {
                            op: *op,
                            key: key.clone(),
                            value: Box::new(Expr::Var(var.clone())),
                        });
                        acc.extend(pairs.iter().cloned());
                        let map = Expr::Map(Map {
                            span: map.span,
                            annotations: map.annotations.clone(),
                            arg: map.arg.clone(),
                            pairs: acc,
                            is_pattern: map.is_pattern,
                        });
                        let split = Split::new(vec![Expr::Var(var)], p, split);
                        Some((map, Some(split)))
                    }
                },
                key => {
                    let span = key.span();
                    let keyvar = self.context_mut().next_var(Some(span));
                    let mapvar = self.context_mut().next_var(Some(span));
                    let e = MapPair {
                        op: *op,
                        key: Box::new(Expr::Var(keyvar.clone())),
                        value: value.clone(),
                    };
                    let pairs = core::iter::once(e).chain(pairs.iter().cloned()).collect();
                    let aftmap = Map {
                        span: map.span,
                        annotations: map.annotations.clone(),
                        arg: map.arg.clone(),
                        pairs,
                        is_pattern: map.is_pattern,
                    };
                    let (wrap, casearg, aftmap) =
                        self.wrap_map_key_fun(key.clone(), keyvar, mapvar.clone(), aftmap);
                    let split = Some(Split::new_wrapped(vec![casearg], aftmap, Some(wrap), None));
                    let pattern = Box::new(Expr::Map(Map {
                        span: map.span,
                        annotations: map.annotations.clone(),
                        arg: map.arg.clone(),
                        pairs: acc,
                        is_pattern: map.is_pattern,
                    }));
                    let alias = Expr::Alias(Alias {
                        span: map.span,
                        annotations: Annotations::default(),
                        var: mapvar,
                        pattern,
                    });
                    Some((alias, split))
                }
            },
        }
    }

    fn wrap_map_key_fun(
        &mut self,
        key: Expr,
        keyvar: Var,
        mapvar: Var,
        aftmap: Map,
    ) -> (Box<WrapperFn>, Expr, Expr) {
        if key.is_safe() {
            let wrapper = Box::new(|body| {
                Box::new(Expr::Let(Let {
                    span: keyvar.span(),
                    annotations: Annotations::default(),
                    vars: vec![keyvar],
                    arg: Box::new(key),
                    body: Box::new(body),
                }))
            });
            (wrapper, Expr::Var(mapvar), Expr::Map(aftmap))
        } else {
            let span = keyvar.span();
            let succvar = self.context_mut().next_var(Some(span));
            let evars = self.context_mut().next_n_vars(3, Some(span));
            let t = Expr::Literal(Literal::atom(span, symbols::True));
            let mv = Expr::Tuple(Tuple::new(
                span,
                vec![Expr::Var(succvar.clone()), Expr::Var(mapvar.clone())],
            ));
            let aft = Expr::Tuple(Tuple::new(span, vec![t.clone(), Expr::Map(aftmap)]));
            let wrapper = Box::new(|body| {
                let span = keyvar.span();
                let t = Expr::Literal(Literal::atom(span, symbols::True));
                let f1 = Expr::Literal(Literal::atom(span, symbols::False));
                let f2 = Expr::Literal(Literal::atom(span, symbols::False));
                let tbody = Box::new(Values::new(span, vec![t, Expr::Var(keyvar.clone())]));
                let texpr = Box::new(Expr::Try(Try {
                    span,
                    annotations: Annotations::default(),
                    arg: Box::new(key),
                    vars: vec![keyvar.clone()],
                    body: tbody,
                    evars,
                    handler: Box::new(Values::new(span, vec![f1, f2])),
                }));
                Box::new(Expr::Let(Let {
                    span,
                    annotations: Annotations::default(),
                    vars: vec![succvar, keyvar],
                    arg: texpr,
                    body: Box::new(body),
                }))
            });
            (wrapper, mv, aft)
        }
    }

    fn split_data(
        &mut self,
        mut elements: &[Expr],
        mut acc: Vec<Expr>,
    ) -> Option<(Vec<Expr>, Option<Box<Split>>)> {
        match elements.split_off_first() {
            None => None,
            Some(e) => match self.split_pat(e) {
                None => {
                    acc.push(e.clone());
                    self.split_data(elements, acc)
                }
                Some((p, split)) => {
                    let var = self.context_mut().next_var(Some(e.span()));
                    acc.push(Expr::Var(var.clone()));
                    acc.extend(elements.iter().cloned());
                    Some((acc, Some(Split::new(vec![Expr::Var(var)], p, split))))
                }
            },
        }
    }

    fn split_bin_segments(
        &mut self,
        mut segments: &[Bitstring],
        vars0: RedBlackTreeSet<Ident>,
        mut acc: Vec<Bitstring>,
    ) -> Option<(Var, Option<Box<WrapperFn>>, Vec<Bitstring>, Vec<Bitstring>)> {
        let osegments = &segments[..];
        match segments.split_off_first() {
            None => None,
            Some(s0) => {
                let vars = match s0.value.as_ref() {
                    Expr::Var(v) => vars0.insert(v.name),
                    _ => vars0.clone(),
                };
                match s0.size.as_deref() {
                    None | Some(Expr::Literal(_)) => {
                        acc.push(s0.clone());
                        self.split_bin_segments(segments, vars, acc)
                    }
                    Some(Expr::Var(v)) => {
                        if vars.contains(&v.name) {
                            // The size variable is variable previously bound
                            // in this same segment. Split the clause here to
                            // avoid a variable that is both defined and used in
                            // the same pattern.
                            let (tail_var, tail) =
                                self.split_tail_seg(s0.span, s0.annotations.clone(), osegments);
                            acc.push(tail);
                            Some((tail_var, None, acc, osegments.iter().cloned().collect()))
                        } else {
                            acc.push(s0.clone());
                            self.split_bin_segments(segments, vars, acc)
                        }
                    }
                    Some(sz) => {
                        // The size is an expression. Split the clause here,
                        // calculate the expression in a try/catch, and finally
                        // continue the match in an inner case.
                        let (tail_var, tail) = self.split_tail_seg(s0.span, s0.annotations.clone(), osegments);
                        let size_var = self.context_mut().next_var(Some(sz.span()));
                        let mut segment = s0.clone();
                        segment.size = Some(Box::new(Expr::Var(size_var.clone())));
                        let wrap = self.split_wrap(size_var, Box::new(sz.clone()));
                        acc.push(tail);
                        let segments = core::iter::once(segment)
                            .chain(segments.iter().cloned())
                            .collect();
                        Some((tail_var, Some(wrap), acc, segments))
                    }
                }
            }
        }
    }

    fn split_tail_seg(&mut self, span: SourceSpan, annotations: Annotations, segments: &[Bitstring]) -> (Var, Bitstring) {
        let tail_var = self.context_mut().next_var(Some(span));
        let bs = Bitstring {
            span,
            annotations,
            value: Box::new(Expr::Var(tail_var.clone())),
            size: Some(Box::new(Expr::Literal(Literal::atom(span, symbols::All)))),
            spec: BinaryEntrySpecifier::Binary { unit: if self.dialyzer { split_binary_unit(segments) } else { 1 } },
        };
        (tail_var, bs)
    }

    fn split_wrap(&mut self, size_var: Var, size_expr: Box<Expr>) -> Box<WrapperFn> {
        let span = size_var.span();
        let evars = self.context_mut().next_n_vars(3, Some(span));
        Box::new(move |body| {
            let texpr = Box::new(Expr::Try(Try {
                span,
                annotations: Annotations::default(),
                arg: size_expr,
                vars: vec![size_var.clone()],
                body: Box::new(Expr::Var(size_var.clone())),
                evars,
                handler: Box::new(Expr::Literal(Literal::atom(span, symbols::BadSize))),
            }));
            Box::new(Expr::Let(Let {
                span,
                annotations: Annotations::default(),
                vars: vec![size_var],
                arg: texpr,
                body: Box::new(body),
            }))
        })
    }
}

fn split_binary_unit(segments: &[Bitstring]) -> u8 {
    let span = segments[0].span;
    let integer = |n: i64| Literal::integer(span, n).as_integer().unwrap().clone();
    let mut divisor = integer(0);
    for segment in segments {
        let bits = match segment.spec {
            BinaryEntrySpecifier::Utf8 => integer(8),
            BinaryEntrySpecifier::Utf16 { .. } => integer(16),
            BinaryEntrySpecifier::Utf32 { .. } => integer(32),
            _ => match segment.size.as_deref() {
                Some(Expr::Literal(literal)) if literal.is_integer() => {
                    let size = literal.as_integer().unwrap();
                    if size.is_zero() { integer(1) } else { size * segment.spec.unit() }
                }
                _ => integer(segment.spec.unit() as i64),
            },
        };
        let mut right = bits;
        while !right.is_zero() {
            let remainder = (divisor % &right).expect("alignment divisor is nonzero");
            divisor = right;
            right = remainder;
        }
    }
    divisor.to_usize().and_then(|unit| u8::try_from(unit).ok())
        .expect("split suffix starts with a dynamic segment whose unit bounds the alignment")
}

fn split_letify(mut vs: Vec<Expr>, mut args: Vec<Expr>, body: Box<Expr>) -> Expr {
    let mut vsacc = vec![];
    let mut argacc = vec![];
    for pair in vs.drain(..).zip(args.drain(..)) {
        match pair {
            (Expr::Literal(x), Expr::Literal(y)) if x == y => continue,
            (Expr::Literal(x), Expr::Literal(y)) => panic!(
                "mismatched literal pattern in split_letify: {:?} can never match {:?}",
                &x, &y
            ),
            (Expr::Var(x), Expr::Var(y)) if x.name == y.name => continue,
            (Expr::Var(v), arg) => {
                vsacc.push(v);
                argacc.push(arg);
            }
            _ => unreachable!(),
        }
    }
    if vsacc.is_empty() && argacc.is_empty() {
        return *body;
    }
    assert!(!vsacc.is_empty() && !argacc.is_empty());
    let span = body.span();
    Expr::Let(Let {
        span,
        annotations: Annotations::default(),
        vars: vsacc,
        arg: Box::new(Values::new(span, argacc)),
        body,
    })
}

#[cfg(test)]
mod binary_alignment_tests {
    use super::*;
    use firefly_intern::Symbol;
    fn segment(spec: BinaryEntrySpecifier, size: Option<i64>) -> Bitstring {
        let span = SourceSpan::default();
        Bitstring {
            span, annotations: Annotations::default(),
            value: Box::new(Expr::Var(Var::new(Ident::from_str("value")))),
            size: Some(Box::new(match size {
                Some(n) => Expr::Literal(Literal::integer(span, n)),
                None => Expr::Var(Var::new(Ident::from_str("size"))),
            })), spec,
        }
    }
    #[test]
    fn dynamic_binary_suffix_retains_byte_alignment() {
        assert_eq!(split_binary_unit(&[segment(BinaryEntrySpecifier::Binary { unit: 8 }, None),
            segment(BinaryEntrySpecifier::Binary { unit: 8 }, None)]), 8);
    }
    #[test]
    fn mixed_widths_reduce_alignment_and_zero_has_unit_one() {
        let first = segment(BinaryEntrySpecifier::Binary { unit: 8 }, None);
        assert_eq!(split_binary_unit(&[first.clone(), segment(BinaryEntrySpecifier::DEFAULT, Some(12))]), 4);
        assert_eq!(split_binary_unit(&[first, segment(BinaryEntrySpecifier::DEFAULT, Some(0))]), 1);
    }
    #[test]
    fn utf_alignment_uses_encoded_widths() {
        assert_eq!(split_binary_unit(&[segment(BinaryEntrySpecifier::Binary { unit: 24 }, None),
            segment(BinaryEntrySpecifier::Utf8, None)]), 8);
        assert_eq!(split_binary_unit(&[segment(BinaryEntrySpecifier::Binary { unit: 32 }, None),
            segment(BinaryEntrySpecifier::Utf16 { endianness: firefly_binary::Endianness::Big }, None),
            segment(BinaryEntrySpecifier::Utf32 { endianness: firefly_binary::Endianness::Big }, None)]), 16);
    }
    #[test]
    fn ordinary_compiler_mode_keeps_arbitrary_bit_remainder() {
        let span = SourceSpan::default();
        let name = FunctionName::new(Symbol::intern("proof"), Symbol::intern("f"), 1);
        let context = Rc::new(UnsafeCell::new(FunctionContext::new(span, name, 0, 0, false)));
        let segments = [segment(BinaryEntrySpecifier::Binary { unit: 8 }, None)];
        let mut normal = RewriteReceivePrimitives::new(Rc::clone(&context));
        let mut analysis = RewriteReceivePrimitives::new(context).for_dialyzer();
        assert_eq!(normal.split_tail_seg(span, Annotations::default(), &segments).1.spec.unit(), 1);
        assert_eq!(analysis.split_tail_seg(span, Annotations::default(), &segments).1.spec.unit(), 8);
    }
    #[test]
    fn only_literal_infinite_timeout_is_ignored_in_dialyzer_mode() {
        let span = SourceSpan::default();
        let name = FunctionName::new(Symbol::intern("proof"), Symbol::intern("receive"), 0);
        let context = Rc::new(UnsafeCell::new(FunctionContext::new(span, name, 0, 0, false)));
        let ordinary = RewriteReceivePrimitives::new(Rc::clone(&context));
        let analysis = RewriteReceivePrimitives::new(context).for_dialyzer();
        let infinity = Expr::Literal(Literal::atom(span, symbols::Infinity));
        assert!(!ordinary.timeout_is_ignored(&infinity));
        assert!(analysis.timeout_is_ignored(&infinity));
        assert!(!analysis.timeout_is_ignored(&Expr::Literal(Literal::integer(span, 0))));
        assert!(!analysis.timeout_is_ignored(&Expr::Var(Var::new(Ident::from_str("timeout")))));
    }
    #[test]
    fn infinite_receive_switch_marks_only_the_unreachable_source_clause() {
        let span = SourceSpan::default();
        let atom = || Box::new(Expr::Literal(Literal::atom(span, symbols::True)));
        for branch in [true, false] {
            let Expr::Case(case) = RewriteReceivePrimitives::receive_switch(span, Annotations::from([symbols::ReceiveTimeout]), atom(), atom(), atom(), Some(branch)) else { panic!("explicit receive clauses expected") };
            assert!(case.annotations.contains(symbols::ReceiveTimeout));
            assert_eq!(case.clauses[0].annotations.contains(Symbol::intern("dialyzer_ignore")), branch);
            assert_eq!(case.clauses[1].annotations.contains(Symbol::intern("dialyzer_ignore")), !branch);
        }
        assert!(matches!(RewriteReceivePrimitives::receive_switch(span, Annotations::default(), atom(), atom(), atom(), None), Expr::If(_)));
    }

}
