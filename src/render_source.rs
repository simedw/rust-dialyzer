use super::{Clause, Expr, Function, Operator, Pattern, Qualifier};
pub(super) fn source() -> Vec<(Function, usize, Vec<Clause>)> {
    vec![
        (
            Function::Message,
            3,
            vec![
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("apply"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Args")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("ArgNs")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("FailReason")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("SigArgs")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("SigRet")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("Contract")),
                                                    Box::new(Pattern::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Op(
                        Operator::Append,
                        vec![
                            Expr::Call(
                                Function::Format,
                                vec![
                                    Expr::Text("Fun application with arguments ~ts "),
                                    Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("Args"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Nil),
                                    ),
                                ],
                            ),
                            Expr::Call(
                                Function::CallReason,
                                vec![
                                    Expr::Var("ArgNs"),
                                    Expr::Var("FailReason"),
                                    Expr::Var("SigArgs"),
                                    Expr::Var("SigRet"),
                                    Expr::Var("Contract"),
                                    Expr::Var("I"),
                                ],
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("app_call"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Args")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Culprit")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("ExpectedType")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("FoundType")),
                                                    Box::new(Pattern::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The call ~s:~ts~ts requires that ~ts is of type ~ts not ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("Args"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::Code,
                                                vec![Expr::Var("Culprit"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("ExpectedType"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Cons(
                                                    Box::new(Expr::Call(
                                                        Function::TypeLayout,
                                                        vec![
                                                            Expr::Var("FoundType"),
                                                            Expr::Var("I"),
                                                        ],
                                                    )),
                                                    Box::new(Expr::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("bin_construction"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Culprit")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Size")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Seg")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Type")),
                                            Box::new(Pattern::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Binary construction will fail since the ~ts field ~ts in segment ~ts has type ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("Culprit")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Code,
                                        vec![Expr::Var("Size"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Code,
                                            vec![Expr::Var("Seg"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("Type"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("call"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Args")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ArgNs")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("FailReason")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("SigArgs")),
                                                    Box::new(Pattern::Cons(
                                                        Box::new(Pattern::Var("SigRet")),
                                                        Box::new(Pattern::Cons(
                                                            Box::new(Pattern::Var("Contract")),
                                                            Box::new(Pattern::Nil),
                                                        )),
                                                    )),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Op(
                        Operator::Append,
                        vec![
                            Expr::Call(
                                Function::Format,
                                vec![
                                    Expr::Text("The call ~w:~tw~ts "),
                                    Expr::Cons(
                                        Box::new(Expr::Var("M")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Var("F")),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::Arguments,
                                                    vec![Expr::Var("Args"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    ),
                                ],
                            ),
                            Expr::Call(
                                Function::CallReason,
                                vec![
                                    Expr::Var("ArgNs"),
                                    Expr::Var("FailReason"),
                                    Expr::Var("SigArgs"),
                                    Expr::Var("SigRet"),
                                    Expr::Var("Contract"),
                                    Expr::Var("I"),
                                ],
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("call_to_missing"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Call to missing or unexported function ~w:~tw/~w\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("exact_compare"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Type1")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Op")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Type2")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("The test ~ts ~s ~ts can never evaluate to '~w'\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::TypeLayout,
                                    vec![Expr::Var("Type1"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("Op")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::TypeLayout,
                                            vec![Expr::Var("Type2"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Op(
                                                Operator::Or,
                                                vec![
                                                    Expr::Op(
                                                        Operator::Equal,
                                                        vec![Expr::Var("Op"), Expr::Atom("=:=")],
                                                    ),
                                                    Expr::Op(
                                                        Operator::Equal,
                                                        vec![Expr::Var("Op"), Expr::Atom("==")],
                                                    ),
                                                ],
                                            )),
                                            Box::new(Expr::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("fun_app_args"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("ArgNs")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Args")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Type")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Var("PositionString"),
                            Box::new(Expr::Call(Function::Positions, vec![Expr::Var("ArgNs")])),
                        ),
                        Expr::Call(
                            Function::Format,
                            vec![
                                Expr::Text(
                                    "Fun application with arguments ~ts will fail since the function has type ~ts, which differs in the ~s argument\n",
                                ),
                                Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Arguments,
                                        vec![Expr::Var("Args"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::TypeLayout,
                                            vec![Expr::Var("Type"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Var("PositionString")),
                                            Box::new(Expr::Nil),
                                        )),
                                    )),
                                ),
                            ],
                        ),
                    ],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("fun_app_no_fun"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Op")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Type")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Arity")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Fun application will fail since ~ts :: ~ts is not a function of arity ~w\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("Op")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::TypeLayout,
                                        vec![Expr::Var("Type"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("Arity")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![Pattern::Atom("guard_fail"), Pattern::Nil]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Text("Clause guard cannot succeed.\n")],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("guard_fail"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Arg1")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Infix")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Arg2")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Guard test ~ts ~s ~ts can never succeed\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::Arguments,
                                    vec![Expr::Var("Arg1"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("Infix")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("Arg2"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("map_update"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Type")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Key")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("A key of type ~ts cannot exist in a map of type ~ts\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::TypeLayout,
                                    vec![Expr::Var("Key"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::TypeLayout,
                                        vec![Expr::Var("Type"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("neg_guard_fail"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Arg1")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Infix")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Arg2")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Guard test not(~ts ~s ~ts) can never succeed\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::Arguments,
                                    vec![Expr::Var("Arg1"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("Infix")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("Arg2"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("guard_fail"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Guard")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Args")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Guard test ~s~ts can never succeed\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("Guard")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Arguments,
                                        vec![Expr::Var("Args"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("neg_guard_fail"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Guard")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Args")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Guard test not(~s~ts) can never succeed\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("Guard")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Arguments,
                                        vec![Expr::Var("Args"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("guard_fail_pat"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Pat")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Type")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Clause guard cannot succeed. The ~ts was matched against the type ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::PatternString,
                                    vec![Expr::Var("Pat"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::TypeLayout,
                                        vec![Expr::Var("Type"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("improper_list_constr"),
                            Pattern::Cons(Box::new(Pattern::Var("TlType")), Box::new(Pattern::Nil)),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Cons will produce an improper list since its 2nd argument is ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::TypeLayout,
                                    vec![Expr::Var("TlType"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Nil),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("no_return"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Type")),
                                Box::new(Pattern::Var("Name")),
                            ),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Var("NameString"),
                            Box::new(Expr::Case(
                                Box::new(Expr::Var("Name")),
                                vec![
                                    Clause {
                                        patterns: vec![Pattern::Nil],
                                        guards: vec![],
                                        body: vec![Expr::Text("The created fun ")],
                                    },
                                    Clause {
                                        patterns: vec![Pattern::Cons(
                                            Box::new(Pattern::Var("F")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("A")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )],
                                        guards: vec![],
                                        body: vec![Expr::Call(
                                            Function::Format,
                                            vec![
                                                Expr::Text("Function ~tw/~w "),
                                                Expr::Cons(
                                                    Box::new(Expr::Var("F")),
                                                    Box::new(Expr::Cons(
                                                        Box::new(Expr::Var("A")),
                                                        Box::new(Expr::Nil),
                                                    )),
                                                ),
                                            ],
                                        )],
                                    },
                                ],
                            )),
                        ),
                        Expr::Case(
                            Box::new(Expr::Var("Type")),
                            vec![
                                Clause {
                                    patterns: vec![Pattern::Atom("no_match")],
                                    guards: vec![],
                                    body: vec![Expr::Op(
                                        Operator::Append,
                                        vec![
                                            Expr::Var("NameString"),
                                            Expr::Text("has no clauses that will ever match\n"),
                                        ],
                                    )],
                                },
                                Clause {
                                    patterns: vec![Pattern::Atom("only_explicit")],
                                    guards: vec![],
                                    body: vec![Expr::Op(
                                        Operator::Append,
                                        vec![
                                            Expr::Var("NameString"),
                                            Expr::Text("only terminates with explicit exception\n"),
                                        ],
                                    )],
                                },
                                Clause {
                                    patterns: vec![Pattern::Atom("only_normal")],
                                    guards: vec![],
                                    body: vec![Expr::Op(
                                        Operator::Append,
                                        vec![
                                            Expr::Var("NameString"),
                                            Expr::Text("has no local return\n"),
                                        ],
                                    )],
                                },
                                Clause {
                                    patterns: vec![Pattern::Atom("both")],
                                    guards: vec![],
                                    body: vec![Expr::Op(
                                        Operator::Append,
                                        vec![
                                            Expr::Var("NameString"),
                                            Expr::Text("has no local return\n"),
                                        ],
                                    )],
                                },
                            ],
                        ),
                    ],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("record_constr"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("RecConstr")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("FieldDiffs")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Record construction ~ts violates the declared type of field ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::TypeLayout,
                                    vec![Expr::Var("RecConstr"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::FieldDiffs,
                                        vec![Expr::Var("FieldDiffs"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("record_constr"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Name")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Field")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Type")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Record construction violates the declared type for #~tw{} since ~ts cannot be of type ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("Name")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::PatternString,
                                        vec![Expr::Var("Field"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::TypeLayout,
                                            vec![Expr::Var("Type"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("record_matching"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("String")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Name")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("The ~ts violates the declared type for #~tw{}\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::RecordString,
                                    vec![Expr::Var("String"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("Name")),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("record_match"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Pat")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Type")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Matching of ~ts tagged with a record name violates the declared type of ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::PatternString,
                                    vec![Expr::Var("Pat"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::TypeLayout,
                                        vec![Expr::Var("Type"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("pattern_match"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Pat")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Type")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("The ~ts can never match the type ~ts\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::PatternString,
                                    vec![Expr::Var("Pat"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::TypeLayout,
                                        vec![Expr::Var("Type"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("pattern_match_cov"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Pat")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Type")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The ~ts can never match since previous clauses completely covered the type ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::PatternString,
                                    vec![Expr::Var("Pat"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::TypeLayout,
                                        vec![Expr::Var("Type"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("unmatched_return"),
                            Pattern::Cons(Box::new(Pattern::Var("Type")), Box::new(Pattern::Nil)),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Expression produces a value of type ~ts, but this value is unmatched\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::TypeLayout,
                                    vec![Expr::Var("Type"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Nil),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("unused_fun"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("F")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("A")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Function ~tw/~w will never be called\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("F")),
                                Box::new(Expr::Cons(Box::new(Expr::Var("A")), Box::new(Expr::Nil))),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("contract_diff"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("_A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Contract")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("Sig")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Type specification ~ts is not equal to the success typing: ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::Contract,
                                    vec![
                                        Expr::Var("M"),
                                        Expr::Var("F"),
                                        Expr::Var("Contract"),
                                        Expr::Var("I"),
                                    ],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Contract,
                                        vec![
                                            Expr::Var("M"),
                                            Expr::Var("F"),
                                            Expr::Var("Sig"),
                                            Expr::Var("I"),
                                        ],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("contract_subtype"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("_A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Contract")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("Sig")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Type specification ~ts is a subtype of the success typing: ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::Contract,
                                    vec![
                                        Expr::Var("M"),
                                        Expr::Var("F"),
                                        Expr::Var("Contract"),
                                        Expr::Var("I"),
                                    ],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Contract,
                                        vec![
                                            Expr::Var("M"),
                                            Expr::Var("F"),
                                            Expr::Var("Sig"),
                                            Expr::Var("I"),
                                        ],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("contract_supertype"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("_A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Contract")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("Sig")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Type specification ~ts is a supertype of the success typing: ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::Contract,
                                    vec![
                                        Expr::Var("M"),
                                        Expr::Var("F"),
                                        Expr::Var("Contract"),
                                        Expr::Var("I"),
                                    ],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Contract,
                                        vec![
                                            Expr::Var("M"),
                                            Expr::Var("F"),
                                            Expr::Var("Sig"),
                                            Expr::Var("I"),
                                        ],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("contract_range"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Contract")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("M")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("F")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ArgStrings")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("Location")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("CRet")),
                                                    Box::new(Pattern::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The contract ~ts cannot be right because the inferred return for ~tw~ts on position ~s is ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::Contract,
                                    vec![
                                        Expr::Var("M"),
                                        Expr::Var("F"),
                                        Expr::Var("Contract"),
                                        Expr::Var("I"),
                                    ],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("ArgStrings"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::Position,
                                                vec![Expr::Var("Location"), Expr::Var("E")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("CRet"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("invalid_contract"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Atom("none")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("Contract")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("Sig")),
                                                    Box::new(Pattern::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Invalid type specification for function ~w:~tw/~w.\n The success typing is ~ts\n But the spec is ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::Contract,
                                                vec![
                                                    Expr::Var("M"),
                                                    Expr::Var("F"),
                                                    Expr::Var("Sig"),
                                                    Expr::Var("I"),
                                                ],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::Contract,
                                                    vec![
                                                        Expr::Var("M"),
                                                        Expr::Var("F"),
                                                        Expr::Var("Contract"),
                                                        Expr::Var("I"),
                                                    ],
                                                )),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("invalid_contract"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("InvalidContractDetails")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("Contract")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("Sig")),
                                                    Box::new(Pattern::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Invalid type specification for function ~w:~tw/~w.\n The success typing is ~ts\n But the spec is ~ts\n~ts",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::Contract,
                                                vec![
                                                    Expr::Var("M"),
                                                    Expr::Var("F"),
                                                    Expr::Var("Sig"),
                                                    Expr::Var("I"),
                                                ],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::Contract,
                                                    vec![
                                                        Expr::Var("M"),
                                                        Expr::Var("F"),
                                                        Expr::Var("Contract"),
                                                        Expr::Var("I"),
                                                    ],
                                                )),
                                                Box::new(Expr::Cons(
                                                    Box::new(Expr::Call(
                                                        Function::InvalidDetails,
                                                        vec![Expr::Var("InvalidContractDetails")],
                                                    )),
                                                    Box::new(Expr::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("contract_with_opaque"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("OpaqueType")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("SigType")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The specification for ~w:~tw/~w has an opaque subtype ~ts which is violated by the success typing ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("OpaqueType"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::Signature,
                                                    vec![Expr::Var("SigType"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("extra_range"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ExtraRanges")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("SigRange")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The specification for ~w:~tw/~w states that the function might also return ~ts but the inferred return is ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("ExtraRanges"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("SigRange"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("missing_range"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ExtraRanges")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("ContrRange")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The success typing for ~w:~tw/~w implies that the function might also return ~ts but the specification return is ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("ExtraRanges"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("ContrRange"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("overlapping_contract"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Overloaded contract for ~w:~tw/~w has overlapping domains; such contracts cannot establish a dependency between the overloaded input and output types\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("spec_missing_fun"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Contract for function that does not exist: ~w:~tw/~w\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("call_with_opaque"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Args")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Conflicts")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("ExpectedTypes")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Var("Positions"),
                            Box::new(Expr::Comprehension(
                                Box::new(Expr::Var("N")),
                                vec![Qualifier::Generate(
                                    Pattern::Tuple(vec![
                                        Pattern::Var("N"),
                                        Pattern::Var("_T"),
                                        Pattern::Var("_TStr"),
                                    ]),
                                    Expr::Var("Conflicts"),
                                )],
                            )),
                        ),
                        Expr::Call(
                            Function::Format,
                            vec![
                                Expr::Text("The call ~w:~tw~ts contains ~ts when ~ts\n"),
                                Expr::Cons(
                                    Box::new(Expr::Var("M")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("F")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::Arguments,
                                                vec![Expr::Var("Args"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::OpaquePositions,
                                                    vec![Expr::Var("Positions")],
                                                )),
                                                Box::new(Expr::Cons(
                                                    Box::new(Expr::Call(
                                                        Function::Expected,
                                                        vec![
                                                            Expr::Var("ExpectedTypes"),
                                                            Expr::Var("I"),
                                                        ],
                                                    )),
                                                    Box::new(Expr::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                ),
                            ],
                        ),
                    ],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("call_without_opaque"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Args")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Conflicts")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("_ExpectedTypes")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("The call ~w:~tw~ts does not have ~ts\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("Args"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::ExpectedWithoutOpaque,
                                                vec![Expr::Var("Conflicts"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_compare"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Type")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Op")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("OpaqueType")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Var("Kind"),
                            Box::new(Expr::If(vec![
                                Clause {
                                    patterns: vec![],
                                    guards: vec![
                                        vec![Expr::Op(
                                            Operator::Equal,
                                            vec![Expr::Var("Op"), Expr::Atom("=:=")],
                                        )],
                                        vec![Expr::Op(
                                            Operator::Equal,
                                            vec![Expr::Var("Op"), Expr::Atom("==")],
                                        )],
                                    ],
                                    body: vec![Expr::Text("equality")],
                                },
                                Clause {
                                    patterns: vec![],
                                    guards: vec![
                                        vec![Expr::Op(
                                            Operator::Equal,
                                            vec![Expr::Var("Op"), Expr::Atom("=/=")],
                                        )],
                                        vec![Expr::Op(
                                            Operator::Equal,
                                            vec![Expr::Var("Op"), Expr::Atom("/=")],
                                        )],
                                    ],
                                    body: vec![Expr::Text("inequality")],
                                },
                            ])),
                        ),
                        Expr::Call(
                            Function::Format,
                            vec![
                                Expr::Text(
                                    "Attempt to test for ~ts between a term of type ~ts and a term of opaque type ~ts\n",
                                ),
                                Expr::Cons(
                                    Box::new(Expr::Var("Kind")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::TypeLayout,
                                            vec![Expr::Var("Type"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("OpaqueType"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Nil),
                                        )),
                                    )),
                                ),
                            ],
                        ),
                    ],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_guard"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Arg1")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Infix")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Arg2")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ArgNs")),
                                            Box::new(Pattern::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Guard test ~ts ~s ~ts contains ~s\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::Arguments,
                                    vec![Expr::Var("Arg1"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("Infix")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("Arg2"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::OpaquePositions,
                                                vec![Expr::Var("ArgNs")],
                                            )),
                                            Box::new(Expr::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_guard"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Guard")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Args")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Guard test ~w~ts breaks the opacity of its argument\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("Guard")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Arguments,
                                        vec![Expr::Var("Args"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_match"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Pat")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("OpaqueType")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("OpaqueTerm")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Var("Term"),
                            Box::new(Expr::If(vec![
                                Clause {
                                    patterns: vec![],
                                    guards: vec![vec![Expr::Op(
                                        Operator::Equal,
                                        vec![Expr::Var("OpaqueType"), Expr::Var("OpaqueTerm")],
                                    )]],
                                    body: vec![Expr::Text("the term")],
                                },
                                Clause {
                                    patterns: vec![],
                                    guards: vec![vec![Expr::Atom("true")]],
                                    body: vec![Expr::Op(
                                        Operator::Append,
                                        vec![
                                            Expr::Text("a term of type "),
                                            Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("OpaqueTerm"), Expr::Var("I")],
                                            ),
                                        ],
                                    )],
                                },
                            ])),
                        ),
                        Expr::Call(
                            Function::Format,
                            vec![
                                Expr::Text(
                                    "The attempt to match ~ts against the ~ts breaks the opacity of the term\n",
                                ),
                                Expr::Cons(
                                    Box::new(Expr::Var("Term")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::PatternString,
                                            vec![Expr::Var("Pat"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Nil),
                                    )),
                                ),
                            ],
                        ),
                    ],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_union"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("IsOpaque")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Type")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Var("TypeString"),
                            Box::new(Expr::Call(
                                Function::TypeLayout,
                                vec![Expr::Var("Type"), Expr::Var("I")],
                            )),
                        ),
                        Expr::Case(
                            Box::new(Expr::Var("IsOpaque")),
                            vec![
                                Clause {
                                    patterns: vec![Pattern::Atom("true")],
                                    guards: vec![],
                                    body: vec![Expr::Call(
                                        Function::Format,
                                        vec![
                                            Expr::Text(
                                                "Body yields the opaque type ~ts whose opacity is broken by the other clauses.\n",
                                            ),
                                            Expr::Cons(
                                                Box::new(Expr::Var("TypeString")),
                                                Box::new(Expr::Nil),
                                            ),
                                        ],
                                    )],
                                },
                                Clause {
                                    patterns: vec![Pattern::Atom("false")],
                                    guards: vec![],
                                    body: vec![Expr::Call(
                                        Function::Format,
                                        vec![
                                            Expr::Text(
                                                "Body yields the type ~ts which violates the opacity of the other clauses.\n",
                                            ),
                                            Expr::Cons(
                                                Box::new(Expr::Var("TypeString")),
                                                Box::new(Expr::Nil),
                                            ),
                                        ],
                                    )],
                                },
                            ],
                        ),
                    ],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_type_test"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("Fun")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Args")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Arg")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ArgType")),
                                            Box::new(Pattern::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The type test ~ts~ts breaks the opacity of the term ~ts~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("Fun")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Arguments,
                                        vec![Expr::Var("Args"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("Arg")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("ArgType"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Nil),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_size"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("SizeType")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("Size")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("The size ~ts breaks the opacity of ~ts\n"),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::TypeLayout,
                                    vec![Expr::Var("SizeType"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Call(
                                        Function::Code,
                                        vec![Expr::Var("Size"), Expr::Var("I")],
                                    )),
                                    Box::new(Expr::Nil),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("opaque_call"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("M")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("Args")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("Culprit")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("OpaqueType")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The call ~s:~ts~ts breaks the opacity of the term ~ts :: ~ts\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::Arguments,
                                            vec![Expr::Var("Args"), Expr::Var("I")],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::Code,
                                                vec![Expr::Var("Culprit"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("OpaqueType"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("callback_type_mismatch"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("B")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ST")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("CT")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The inferred return type of ~tw/~w ~ts has nothing in common with ~ts, which is the expected return type for the callback of the ~w behaviour\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("F")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("A")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Call(
                                            Function::TypeLayout,
                                            vec![
                                                Expr::Op(
                                                    Operator::Append,
                                                    vec![
                                                        Expr::Text("("),
                                                        Expr::Op(
                                                            Operator::Append,
                                                            vec![Expr::Var("ST"), Expr::Text(")")],
                                                        ),
                                                    ],
                                                ),
                                                Expr::Var("I"),
                                            ],
                                        )),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("CT"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Var("B")),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("callback_arg_type_mismatch"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("B")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("N")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("ST")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("CT")),
                                                    Box::new(Pattern::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The inferred type for the ~s argument of ~tw/~w (~ts) has nothing in common with ~ts, which is expected type for this argument in the callback of the ~w behaviour\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(Function::Ordinal, vec![Expr::Var("N")])),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("ST"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("CT"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Cons(
                                                    Box::new(Expr::Var("B")),
                                                    Box::new(Expr::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("callback_spec_type_mismatch"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("B")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("ST")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("CT")),
                                                Box::new(Pattern::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The return type ~ts in the specification of ~tw/~w has nothing in common with ~ts, which is the expected return type for the callback of the ~w behaviour\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(
                                    Function::TypeLayout,
                                    vec![Expr::Var("ST"), Expr::Var("I")],
                                )),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("CT"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Var("B")),
                                                Box::new(Expr::Nil),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("callback_spec_arg_type_mismatch"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("B")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("N")),
                                            Box::new(Pattern::Cons(
                                                Box::new(Pattern::Var("ST")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("CT")),
                                                    Box::new(Pattern::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "The specified type for the ~ts argument of ~tw/~w (~ts) has nothing in common with ~ts, which is expected type for this argument in the callback of the ~w behaviour\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Call(Function::Ordinal, vec![Expr::Var("N")])),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("ST"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("CT"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Cons(
                                                    Box::new(Expr::Var("B")),
                                                    Box::new(Expr::Nil),
                                                )),
                                            )),
                                        )),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("callback_missing"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("B")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Undefined callback function ~tw/~w (behaviour ~w)\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("F")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("A")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("B")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("callback_not_exported"),
                            Pattern::Cons(
                                Box::new(Pattern::Var("B")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("F")),
                                    Box::new(Pattern::Cons(
                                        Box::new(Pattern::Var("A")),
                                        Box::new(Pattern::Nil),
                                    )),
                                )),
                            ),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text(
                                "Callback function ~tw/~w exists but is not exported (behaviour ~w)\n",
                            ),
                            Expr::Cons(
                                Box::new(Expr::Var("F")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("A")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("B")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("callback_info_missing"),
                            Pattern::Cons(Box::new(Pattern::Var("B")), Box::new(Pattern::Nil)),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Callback info about the ~w behaviour is not available\n"),
                            Expr::Cons(Box::new(Expr::Var("B")), Box::new(Expr::Nil)),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("unknown_type"),
                            Pattern::Tuple(vec![
                                Pattern::Var("M"),
                                Pattern::Var("F"),
                                Pattern::Var("A"),
                            ]),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Unknown type ~w:~tw/~w\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![
                            Pattern::Atom("unknown_function"),
                            Pattern::Tuple(vec![
                                Pattern::Var("M"),
                                Pattern::Var("F"),
                                Pattern::Var("A"),
                            ]),
                        ]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Unknown function ~w:~tw/~w\n"),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("A")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::Tuple(vec![Pattern::Atom("unknown_behaviour"), Pattern::Var("B")]),
                        Pattern::Var("_I"),
                        Pattern::Var("_E"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("Unknown behaviour ~w\n"),
                            Expr::Cons(Box::new(Expr::Var("B")), Box::new(Expr::Nil)),
                        ],
                    )],
                },
            ],
        ),
        (
            Function::InvalidDetails,
            1,
            vec![Clause {
                patterns: vec![Pattern::Tuple(vec![
                    Pattern::Var("InvalidArgIdxs"),
                    Pattern::Var("IsRangeInvalid"),
                ])],
                guards: vec![],
                body: vec![
                    Expr::Match(
                        Pattern::Var("ArgOrd"),
                        Box::new(Expr::Call(
                            Function::Positions,
                            vec![Expr::Var("InvalidArgIdxs")],
                        )),
                    ),
                    Expr::Match(
                        Pattern::Var("ArgDesc"),
                        Box::new(Expr::Case(
                            Box::new(Expr::Var("InvalidArgIdxs")),
                            vec![
                                Clause {
                                    patterns: vec![Pattern::Nil],
                                    guards: vec![],
                                    body: vec![Expr::Text("")],
                                },
                                Clause {
                                    patterns: vec![Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Nil),
                                    )],
                                    guards: vec![],
                                    body: vec![Expr::Call(
                                        Function::Format,
                                        vec![
                                            Expr::Text("They do not overlap in the ~ts argument"),
                                            Expr::Cons(
                                                Box::new(Expr::Var("ArgOrd")),
                                                Box::new(Expr::Nil),
                                            ),
                                        ],
                                    )],
                                },
                                Clause {
                                    patterns: vec![Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Var("_")),
                                    )],
                                    guards: vec![],
                                    body: vec![Expr::Call(
                                        Function::Format,
                                        vec![
                                            Expr::Text("They do not overlap in the ~ts arguments"),
                                            Expr::Cons(
                                                Box::new(Expr::Var("ArgOrd")),
                                                Box::new(Expr::Nil),
                                            ),
                                        ],
                                    )],
                                },
                            ],
                        )),
                    ),
                    Expr::Match(
                        Pattern::Var("RangeDesc"),
                        Box::new(Expr::Case(
                            Box::new(Expr::Var("IsRangeInvalid")),
                            vec![
                                Clause {
                                    patterns: vec![Pattern::Atom("true")],
                                    guards: vec![],
                                    body: vec![Expr::Text("return types do not overlap")],
                                },
                                Clause {
                                    patterns: vec![Pattern::Atom("false")],
                                    guards: vec![],
                                    body: vec![Expr::Text("")],
                                },
                            ],
                        )),
                    ),
                    Expr::Case(
                        Box::new(Expr::Tuple(vec![
                            Expr::Var("ArgDesc"),
                            Expr::Var("RangeDesc"),
                        ])),
                        vec![
                            Clause {
                                patterns: vec![Pattern::Tuple(vec![
                                    Pattern::Text(""),
                                    Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Var("_")),
                                    ),
                                ])],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text(" The ~ts\n"),
                                        Expr::Cons(
                                            Box::new(Expr::Var("RangeDesc")),
                                            Box::new(Expr::Nil),
                                        ),
                                    ],
                                )],
                            },
                            Clause {
                                patterns: vec![Pattern::Tuple(vec![
                                    Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Var("_")),
                                    ),
                                    Pattern::Text(""),
                                ])],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text(" ~ts\n"),
                                        Expr::Cons(
                                            Box::new(Expr::Var("ArgDesc")),
                                            Box::new(Expr::Nil),
                                        ),
                                    ],
                                )],
                            },
                            Clause {
                                patterns: vec![Pattern::Tuple(vec![
                                    Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Var("_")),
                                    ),
                                    Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Var("_")),
                                    ),
                                ])],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text(" ~ts, and the ~ts\n"),
                                        Expr::Cons(
                                            Box::new(Expr::Var("ArgDesc")),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Var("RangeDesc")),
                                                Box::new(Expr::Nil),
                                            )),
                                        ),
                                    ],
                                )],
                            },
                        ],
                    ),
                ],
            }],
        ),
        (
            Function::CallReason,
            6,
            vec![Clause {
                patterns: vec![
                    Pattern::Var("ArgNs"),
                    Pattern::Var("FailReason"),
                    Pattern::Var("SigArgs"),
                    Pattern::Var("SigRet"),
                    Pattern::Tuple(vec![Pattern::Var("IsOverloaded"), Pattern::Var("Contract")]),
                    Pattern::Var("I"),
                ],
                guards: vec![],
                body: vec![
                    Expr::Match(
                        Pattern::Var("PositionString"),
                        Box::new(Expr::Call(Function::Positions, vec![Expr::Var("ArgNs")])),
                    ),
                    Expr::Case(
                        Box::new(Expr::Var("FailReason")),
                        vec![
                            Clause {
                                patterns: vec![Pattern::Atom("only_sig")],
                                guards: vec![],
                                body: vec![Expr::Case(
                                    Box::new(Expr::Op(
                                        Operator::Equal,
                                        vec![Expr::Var("ArgNs"), Expr::Nil],
                                    )),
                                    vec![
                                        Clause {
                                            patterns: vec![Pattern::Atom("true")],
                                            guards: vec![],
                                            body: vec![Expr::Call(
                                                Function::Format,
                                                vec![
                                                    Expr::Text(
                                                        "will never return since the success typing arguments are ~ts\n",
                                                    ),
                                                    Expr::Cons(
                                                        Box::new(Expr::Call(
                                                            Function::TypeLayout,
                                                            vec![
                                                                Expr::Var("SigArgs"),
                                                                Expr::Var("I"),
                                                            ],
                                                        )),
                                                        Box::new(Expr::Nil),
                                                    ),
                                                ],
                                            )],
                                        },
                                        Clause {
                                            patterns: vec![Pattern::Atom("false")],
                                            guards: vec![],
                                            body: vec![Expr::Call(
                                                Function::Format,
                                                vec![
                                                    Expr::Text(
                                                        "will never return since it differs in the ~s argument from the success typing arguments: ~ts\n",
                                                    ),
                                                    Expr::Cons(
                                                        Box::new(Expr::Var("PositionString")),
                                                        Box::new(Expr::Cons(
                                                            Box::new(Expr::Call(
                                                                Function::TypeLayout,
                                                                vec![
                                                                    Expr::Var("SigArgs"),
                                                                    Expr::Var("I"),
                                                                ],
                                                            )),
                                                            Box::new(Expr::Nil),
                                                        )),
                                                    ),
                                                ],
                                            )],
                                        },
                                    ],
                                )],
                            },
                            Clause {
                                patterns: vec![Pattern::Atom("only_contract")],
                                guards: vec![],
                                body: vec![Expr::Case(
                                    Box::new(Expr::Op(
                                        Operator::Or,
                                        vec![
                                            Expr::Op(
                                                Operator::Equal,
                                                vec![Expr::Var("ArgNs"), Expr::Nil],
                                            ),
                                            Expr::Var("IsOverloaded"),
                                        ],
                                    )),
                                    vec![
                                        Clause {
                                            patterns: vec![Pattern::Atom("true")],
                                            guards: vec![],
                                            body: vec![Expr::Call(
                                                Function::Format,
                                                vec![
                                                    Expr::Text("breaks the contract ~ts\n"),
                                                    Expr::Cons(
                                                        Box::new(Expr::Call(
                                                            Function::Signature,
                                                            vec![
                                                                Expr::Var("Contract"),
                                                                Expr::Var("I"),
                                                            ],
                                                        )),
                                                        Box::new(Expr::Nil),
                                                    ),
                                                ],
                                            )],
                                        },
                                        Clause {
                                            patterns: vec![Pattern::Atom("false")],
                                            guards: vec![],
                                            body: vec![Expr::Call(
                                                Function::Format,
                                                vec![
                                                    Expr::Text(
                                                        "breaks the contract ~ts in the ~s argument\n",
                                                    ),
                                                    Expr::Cons(
                                                        Box::new(Expr::Call(
                                                            Function::Signature,
                                                            vec![
                                                                Expr::Var("Contract"),
                                                                Expr::Var("I"),
                                                            ],
                                                        )),
                                                        Box::new(Expr::Cons(
                                                            Box::new(Expr::Var("PositionString")),
                                                            Box::new(Expr::Nil),
                                                        )),
                                                    ),
                                                ],
                                            )],
                                        },
                                    ],
                                )],
                            },
                            Clause {
                                patterns: vec![Pattern::Atom("both")],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text(
                                            "will never return since the success typing is ~ts -> ~ts and the contract is ~ts\n",
                                        ),
                                        Expr::Cons(
                                            Box::new(Expr::Call(
                                                Function::TypeLayout,
                                                vec![Expr::Var("SigArgs"), Expr::Var("I")],
                                            )),
                                            Box::new(Expr::Cons(
                                                Box::new(Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("SigRet"), Expr::Var("I")],
                                                )),
                                                Box::new(Expr::Cons(
                                                    Box::new(Expr::Call(
                                                        Function::Signature,
                                                        vec![Expr::Var("Contract"), Expr::Var("I")],
                                                    )),
                                                    Box::new(Expr::Nil),
                                                )),
                                            )),
                                        ),
                                    ],
                                )],
                            },
                        ],
                    ),
                ],
            }],
        ),
        (
            Function::OpaquePositions,
            1,
            vec![Clause {
                patterns: vec![Pattern::Var("ArgNs")],
                guards: vec![],
                body: vec![Expr::Op(
                    Operator::Append,
                    vec![
                        Expr::Case(
                            Box::new(Expr::Var("ArgNs")),
                            vec![
                                Clause {
                                    patterns: vec![Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Nil),
                                    )],
                                    guards: vec![],
                                    body: vec![Expr::Text("an opaque term as ")],
                                },
                                Clause {
                                    patterns: vec![Pattern::Cons(
                                        Box::new(Pattern::Var("_")),
                                        Box::new(Pattern::Cons(
                                            Box::new(Pattern::Var("_")),
                                            Box::new(Pattern::Var("_")),
                                        )),
                                    )],
                                    guards: vec![],
                                    body: vec![Expr::Text("opaque terms as ")],
                                },
                            ],
                        ),
                        Expr::Op(
                            Operator::Append,
                            vec![
                                Expr::Call(Function::Positions, vec![Expr::Var("ArgNs")]),
                                Expr::Case(
                                    Box::new(Expr::Var("ArgNs")),
                                    vec![
                                        Clause {
                                            patterns: vec![Pattern::Cons(
                                                Box::new(Pattern::Var("_")),
                                                Box::new(Pattern::Nil),
                                            )],
                                            guards: vec![],
                                            body: vec![Expr::Text(" argument")],
                                        },
                                        Clause {
                                            patterns: vec![Pattern::Cons(
                                                Box::new(Pattern::Var("_")),
                                                Box::new(Pattern::Cons(
                                                    Box::new(Pattern::Var("_")),
                                                    Box::new(Pattern::Var("_")),
                                                )),
                                            )],
                                            guards: vec![],
                                            body: vec![Expr::Text(" arguments")],
                                        },
                                    ],
                                ),
                            ],
                        ),
                    ],
                )],
            }],
        ),
        (
            Function::ExpectedWithoutOpaque,
            2,
            vec![
                Clause {
                    patterns: vec![
                        Pattern::Cons(
                            Box::new(Pattern::Tuple(vec![
                                Pattern::Var("N"),
                                Pattern::Var("T"),
                                Pattern::Var("TStr"),
                            ])),
                            Box::new(Pattern::Nil),
                        ),
                        Pattern::Var("I"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Op(
                        Operator::Append,
                        vec![
                            Expr::Case(
                                Box::new(Expr::Call(Function::IsOpaque, vec![Expr::Var("T")])),
                                vec![
                                    Clause {
                                        patterns: vec![Pattern::Atom("true")],
                                        guards: vec![],
                                        body: vec![Expr::Call(
                                            Function::Format,
                                            vec![
                                                Expr::Text("an opaque term of type ~ts as "),
                                                Expr::Cons(
                                                    Box::new(Expr::Call(
                                                        Function::TypeLayout,
                                                        vec![Expr::Var("TStr"), Expr::Var("I")],
                                                    )),
                                                    Box::new(Expr::Nil),
                                                ),
                                            ],
                                        )],
                                    },
                                    Clause {
                                        patterns: vec![Pattern::Atom("false")],
                                        guards: vec![],
                                        body: vec![Expr::Call(
                                            Function::Format,
                                            vec![
                                                Expr::Text(
                                                    "a term of type ~ts (with opaque subterms) as ",
                                                ),
                                                Expr::Cons(
                                                    Box::new(Expr::Call(
                                                        Function::TypeLayout,
                                                        vec![Expr::Var("TStr"), Expr::Var("I")],
                                                    )),
                                                    Box::new(Expr::Nil),
                                                ),
                                            ],
                                        )],
                                    },
                                ],
                            ),
                            Expr::Op(
                                Operator::Append,
                                vec![
                                    Expr::Call(
                                        Function::Positions,
                                        vec![Expr::Cons(
                                            Box::new(Expr::Var("N")),
                                            Box::new(Expr::Nil),
                                        )],
                                    ),
                                    Expr::Text(" argument"),
                                ],
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![Pattern::Var("Conflicts"), Pattern::Var("_I")],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Var("ArgNs"),
                            Box::new(Expr::Comprehension(
                                Box::new(Expr::Var("N")),
                                vec![Qualifier::Generate(
                                    Pattern::Tuple(vec![
                                        Pattern::Var("N"),
                                        Pattern::Var("_T"),
                                        Pattern::Var("_TStr"),
                                    ]),
                                    Expr::Var("Conflicts"),
                                )],
                            )),
                        ),
                        Expr::Op(
                            Operator::Append,
                            vec![
                                Expr::Text("opaque terms as "),
                                Expr::Op(
                                    Operator::Append,
                                    vec![
                                        Expr::Call(Function::Positions, vec![Expr::Var("ArgNs")]),
                                        Expr::Text(" arguments"),
                                    ],
                                ),
                            ],
                        ),
                    ],
                },
            ],
        ),
        (
            Function::Expected,
            2,
            vec![Clause {
                patterns: vec![Pattern::Var("ExpectedArgs"), Pattern::Var("I")],
                guards: vec![],
                body: vec![Expr::Case(
                    Box::new(Expr::Var("ExpectedArgs")),
                    vec![
                        Clause {
                            patterns: vec![Pattern::Cons(
                                Box::new(Pattern::Var("T")),
                                Box::new(Pattern::Nil),
                            )],
                            guards: vec![],
                            body: vec![
                                Expr::Match(
                                    Pattern::Var("TS"),
                                    Box::new(Expr::Call(
                                        Function::TypeString,
                                        vec![Expr::Var("T")],
                                    )),
                                ),
                                Expr::Case(
                                    Box::new(Expr::Call(Function::IsOpaque, vec![Expr::Var("T")])),
                                    vec![
                                        Clause {
                                            patterns: vec![Pattern::Atom("true")],
                                            guards: vec![],
                                            body: vec![Expr::Call(
                                                Function::Format,
                                                vec![
                                                    Expr::Text(
                                                        "an opaque term of type ~ts is expected",
                                                    ),
                                                    Expr::Cons(
                                                        Box::new(Expr::Call(
                                                            Function::TypeLayout,
                                                            vec![Expr::Var("TS"), Expr::Var("I")],
                                                        )),
                                                        Box::new(Expr::Nil),
                                                    ),
                                                ],
                                            )],
                                        },
                                        Clause {
                                            patterns: vec![Pattern::Atom("false")],
                                            guards: vec![],
                                            body: vec![Expr::Call(
                                                Function::Format,
                                                vec![
                                                    Expr::Text(
                                                        "a structured term of type ~ts is expected",
                                                    ),
                                                    Expr::Cons(
                                                        Box::new(Expr::Call(
                                                            Function::TypeLayout,
                                                            vec![Expr::Var("TS"), Expr::Var("I")],
                                                        )),
                                                        Box::new(Expr::Nil),
                                                    ),
                                                ],
                                            )],
                                        },
                                    ],
                                ),
                            ],
                        },
                        Clause {
                            patterns: vec![Pattern::Cons(
                                Box::new(Pattern::Var("_")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("_")),
                                    Box::new(Pattern::Var("_")),
                                )),
                            )],
                            guards: vec![],
                            body: vec![Expr::Text(
                                "terms of different types are expected in these positions",
                            )],
                        },
                    ],
                )],
            }],
        ),
        (
            Function::Positions,
            1,
            vec![Clause {
                patterns: vec![Pattern::Var("ArgNs")],
                guards: vec![],
                body: vec![Expr::Case(
                    Box::new(Expr::Var("ArgNs")),
                    vec![
                        Clause {
                            patterns: vec![Pattern::Nil],
                            guards: vec![],
                            body: vec![Expr::Text("")],
                        },
                        Clause {
                            patterns: vec![Pattern::Cons(
                                Box::new(Pattern::Var("N1")),
                                Box::new(Pattern::Nil),
                            )],
                            guards: vec![],
                            body: vec![Expr::Call(Function::Ordinal, vec![Expr::Var("N1")])],
                        },
                        Clause {
                            patterns: vec![Pattern::Cons(
                                Box::new(Pattern::Var("_")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("_")),
                                    Box::new(Pattern::Var("_")),
                                )),
                            )],
                            guards: vec![],
                            body: vec![
                                Expr::Match(
                                    Pattern::Cons(
                                        Box::new(Pattern::Var("Last")),
                                        Box::new(Pattern::Var("Prevs")),
                                    ),
                                    Box::new(Expr::Call(
                                        Function::Reverse,
                                        vec![Expr::Var("ArgNs")],
                                    )),
                                ),
                                Expr::Match(
                                    Pattern::Prefix(
                                        Box::new(Pattern::Text(", ")),
                                        Box::new(Pattern::Var("Head")),
                                    ),
                                    Box::new(Expr::Call(
                                        Function::Flatten,
                                        vec![Expr::Comprehension(
                                            Box::new(Expr::Call(
                                                Function::Format,
                                                vec![
                                                    Expr::Text(", ~s"),
                                                    Expr::Cons(
                                                        Box::new(Expr::Call(
                                                            Function::Ordinal,
                                                            vec![Expr::Var("N")],
                                                        )),
                                                        Box::new(Expr::Nil),
                                                    ),
                                                ],
                                            )),
                                            vec![Qualifier::Generate(
                                                Pattern::Var("N"),
                                                Expr::Call(
                                                    Function::Reverse,
                                                    vec![Expr::Var("Prevs")],
                                                ),
                                            )],
                                        )],
                                    )),
                                ),
                                Expr::Op(
                                    Operator::Append,
                                    vec![
                                        Expr::Var("Head"),
                                        Expr::Op(
                                            Operator::Append,
                                            vec![
                                                Expr::Text(" and "),
                                                Expr::Call(
                                                    Function::Ordinal,
                                                    vec![Expr::Var("Last")],
                                                ),
                                            ],
                                        ),
                                    ],
                                ),
                            ],
                        },
                    ],
                )],
            }],
        ),
        (
            Function::Ordinal,
            1,
            vec![
                Clause {
                    patterns: vec![Pattern::Var("N")],
                    guards: vec![vec![
                        Expr::Call(Function::IsInteger, vec![Expr::Var("N")]),
                        Expr::Op(
                            Operator::Or,
                            vec![
                                Expr::Op(
                                    Operator::Equal,
                                    vec![
                                        Expr::Op(
                                            Operator::Remainder,
                                            vec![Expr::Var("N"), Expr::Integer("100")],
                                        ),
                                        Expr::Integer("11"),
                                    ],
                                ),
                                Expr::Op(
                                    Operator::Or,
                                    vec![
                                        Expr::Op(
                                            Operator::Equal,
                                            vec![
                                                Expr::Op(
                                                    Operator::Remainder,
                                                    vec![Expr::Var("N"), Expr::Integer("100")],
                                                ),
                                                Expr::Integer("12"),
                                            ],
                                        ),
                                        Expr::Op(
                                            Operator::Equal,
                                            vec![
                                                Expr::Op(
                                                    Operator::Remainder,
                                                    vec![Expr::Var("N"), Expr::Integer("100")],
                                                ),
                                                Expr::Integer("13"),
                                            ],
                                        ),
                                    ],
                                ),
                            ],
                        ),
                    ]],
                    body: vec![Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("~Bth"),
                            Expr::Cons(Box::new(Expr::Var("N")), Box::new(Expr::Nil)),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![Pattern::Var("N")],
                    guards: vec![vec![Expr::Call(Function::IsInteger, vec![Expr::Var("N")])]],
                    body: vec![Expr::Case(
                        Box::new(Expr::Call(
                            Function::Min,
                            vec![
                                Expr::Op(
                                    Operator::Remainder,
                                    vec![Expr::Var("N"), Expr::Integer("10")],
                                ),
                                Expr::Integer("4"),
                            ],
                        )),
                        vec![
                            Clause {
                                patterns: vec![Pattern::Integer("1")],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text("~Bst"),
                                        Expr::Cons(Box::new(Expr::Var("N")), Box::new(Expr::Nil)),
                                    ],
                                )],
                            },
                            Clause {
                                patterns: vec![Pattern::Integer("2")],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text("~Bnd"),
                                        Expr::Cons(Box::new(Expr::Var("N")), Box::new(Expr::Nil)),
                                    ],
                                )],
                            },
                            Clause {
                                patterns: vec![Pattern::Integer("3")],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text("~Brd"),
                                        Expr::Cons(Box::new(Expr::Var("N")), Box::new(Expr::Nil)),
                                    ],
                                )],
                            },
                            Clause {
                                patterns: vec![Pattern::Var("_")],
                                guards: vec![],
                                body: vec![Expr::Call(
                                    Function::Format,
                                    vec![
                                        Expr::Text("~Bth"),
                                        Expr::Cons(Box::new(Expr::Var("N")), Box::new(Expr::Nil)),
                                    ],
                                )],
                            },
                        ],
                    )],
                },
            ],
        ),
        (
            Function::Contract,
            4,
            vec![Clause {
                patterns: vec![
                    Pattern::Var("M"),
                    Pattern::Var("F"),
                    Pattern::Var("Src"),
                    Pattern::Var("I"),
                ],
                guards: vec![],
                body: vec![
                    Expr::Match(
                        Pattern::Var("S"),
                        Box::new(Expr::Call(
                            Function::Signature,
                            vec![Expr::Var("Src"), Expr::Var("I")],
                        )),
                    ),
                    Expr::Call(
                        Function::Format,
                        vec![
                            Expr::Text("~w:~tw~ts"),
                            Expr::Cons(
                                Box::new(Expr::Var("M")),
                                Box::new(Expr::Cons(
                                    Box::new(Expr::Var("F")),
                                    Box::new(Expr::Cons(
                                        Box::new(Expr::Var("S")),
                                        Box::new(Expr::Nil),
                                    )),
                                )),
                            ),
                        ],
                    ),
                ],
            }],
        ),
        (
            Function::Arguments,
            2,
            vec![
                Clause {
                    patterns: vec![
                        Pattern::All(vec![Pattern::Text(""), Pattern::Var("Args")]),
                        Pattern::Var("_I"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Var("Args")],
                },
                Clause {
                    patterns: vec![Pattern::Var("Args"), Pattern::Var("I")],
                    guards: vec![],
                    body: vec![Expr::Call(
                        Function::TypeLayout,
                        vec![Expr::Var("Args"), Expr::Var("I")],
                    )],
                },
            ],
        ),
        (
            Function::Code,
            2,
            vec![Clause {
                patterns: vec![Pattern::Var("Cerl"), Pattern::Var("_I")],
                guards: vec![],
                body: vec![Expr::Var("Cerl")],
            }],
        ),
        (
            Function::RecordString,
            2,
            vec![Clause {
                patterns: vec![
                    Pattern::Prefix(
                        Box::new(Pattern::Text("record ")),
                        Box::new(Pattern::Var("Src")),
                    ),
                    Pattern::Var("I"),
                ],
                guards: vec![],
                body: vec![Expr::Op(
                    Operator::Append,
                    vec![
                        Expr::Text("record "),
                        Expr::Call(Function::TypeLayout, vec![Expr::Var("Src"), Expr::Var("I")]),
                    ],
                )],
            }],
        ),
        (
            Function::PatternString,
            2,
            vec![
                Clause {
                    patterns: vec![
                        Pattern::Prefix(
                            Box::new(Pattern::Text("pattern ")),
                            Box::new(Pattern::Var("Src")),
                        ),
                        Pattern::Var("I"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Op(
                        Operator::Append,
                        vec![
                            Expr::Text("pattern "),
                            Expr::Call(
                                Function::TypeLayout,
                                vec![Expr::Var("Src"), Expr::Var("I")],
                            ),
                        ],
                    )],
                },
                Clause {
                    patterns: vec![
                        Pattern::All(vec![
                            Pattern::Prefix(
                                Box::new(Pattern::Text("variable ")),
                                Box::new(Pattern::Var("_")),
                            ),
                            Pattern::Var("Src"),
                        ]),
                        Pattern::Var("_I"),
                    ],
                    guards: vec![],
                    body: vec![Expr::Var("Src")],
                },
                Clause {
                    patterns: vec![
                        Pattern::Prefix(
                            Box::new(Pattern::Text("record field")),
                            Box::new(Pattern::Var("Rest")),
                        ),
                        Pattern::Var("I"),
                    ],
                    guards: vec![],
                    body: vec![
                        Expr::Match(
                            Pattern::Cons(
                                Box::new(Pattern::Var("S")),
                                Box::new(Pattern::Cons(
                                    Box::new(Pattern::Var("TypeStr")),
                                    Box::new(Pattern::Nil),
                                )),
                            ),
                            Box::new(Expr::Call(
                                Function::Split,
                                vec![Expr::Var("Rest"), Expr::Text("of type "), Expr::Atom("all")],
                            )),
                        ),
                        Expr::Op(
                            Operator::Append,
                            vec![
                                Expr::Text("record field"),
                                Expr::Op(
                                    Operator::Append,
                                    vec![
                                        Expr::Var("S"),
                                        Expr::Op(
                                            Operator::Append,
                                            vec![
                                                Expr::Text("of type "),
                                                Expr::Call(
                                                    Function::TypeLayout,
                                                    vec![Expr::Var("TypeStr"), Expr::Var("I")],
                                                ),
                                            ],
                                        ),
                                    ],
                                ),
                            ],
                        ),
                    ],
                },
            ],
        ),
    ]
}
