#![cfg(test)]

use Arc from std::sync;

use self from chalk_parse;

use * from crate::errors;
use * from crate::ir;
use {LowerProgram, LowerGoal} from super;

macro_rules! lowering_success {
    (program $program:tt) => {
        let program_text = stringify!($program);
        assert!(program_text.starts_with("{"));
        assert!(program_text.ends_with("}"));
        assert!(parse_and_lower(&program_text[1..program_text.len()-1]).is_ok());
    }
}

macro_rules! lowering_error {
    (program $program:tt error_msg { $expected:expr }) => {
        let program_text = stringify!($program);
        assert!(program_text.starts_with("{"));
        assert!(program_text.ends_with("}"));
        let error = parse_and_lower(&program_text[1..program_text.len()-1]).unwrap_err();
        let expected = Error::from($expected);
        assert_eq!(
            error.to_string(),
            expected.to_string()
        );
    }
}


fn parse_and_lower(text: &str) -> Result<Program> {
    chalk_parse::parse_program(text)?.lower()
}

fn parse_and_lower_goal(program: &Program, text: &str) -> Result<Box<Goal>> {
    chalk_parse::parse_goal(text)?.lower(program)
}

#[test]
fn lower_success() {
    lowering_success! {
        program {
            struct Foo { field: Foo }
            trait Bar { }
            impl Bar for Foo { }
        }
    }
}

#[test]
fn not_trait() {
    lowering_error! {
        program {
            struct Foo { }
            trait Bar { }
            impl Foo for Bar { }
        }
        error_msg {
            "expected a trait, found `Foo`, which is not a trait"
        }
    }
}

#[test]
fn auto_trait() {
    lowering_error! {
        program {
            #[auto] trait Foo<T> { }
        }
        error_msg {
            "auto trait cannot have parameters"
        }
    }

    lowering_error! {
        program {
            trait Bar { }
            #[auto] trait Foo where Self: Bar { }
        }
        error_msg {
            "auto trait cannot have where clauses"
        }
    }

    lowering_error! {
        program {
            #[auto] trait Foo {
                type Item;
            }
        }
        error_msg {
            "auto trait cannot define associated types"
        }
    }

    lowering_success! {
        program {
            #[auto] trait Send { }
        }
    }
}

#[test]
fn negative_impl() {
    lowering_error! {
        program {
            trait Foo {
                type Item;
            }

            struct i32 { }

            impl !Foo for i32 {
                type Item = i32;
            }
        }
        error_msg {
            "negative impls cannot define associated values"
        }
    }

    lowering_success! {
        program {
            trait Foo { }

            trait Iterator {
                type Item;
            }

            struct i32 { }

            impl<T> !Foo for T where T: Iterator<Item = i32> { }
        }
    }
}

#[test]
fn invalid_name() {
    lowering_error! {
        program {
            struct Foo { }
            trait Bar { }
            impl Bar for X { }
        }
        error_msg {
            "invalid type name `X`"
        }
    }
}

#[test]
fn type_parameter() {
    parse_and_lower("struct Foo { } trait Bar { } impl<X> Bar for X { }").unwrap();
}

#[test]
fn type_parameter_bound() {
    parse_and_lower("struct Foo { } trait Bar { } trait Eq { } impl<X> Bar for X where X: Eq { }")
        .unwrap();
}

#[test]
fn assoc_tys() {
    parse_and_lower("
    struct String { }
    struct Char { }

    trait Iterator { type Item; }
    impl Iterator for String { type Item = Char; }

    trait Foo { }
    impl<X> Foo for <X as Iterator>::Item where X: Iterator { }
    ")
        .unwrap();
}

#[test]
fn goal_quantifiers() {
    let program = Arc::new(parse_and_lower("trait Foo<A, B> { }").unwrap());
    let goal = parse_and_lower_goal(&program, "forall<X> {exists<Y> {forall<Z> {Z: Foo<Y, X>}}}")
        .unwrap();
    set_current_program(&program, || {
        assert_eq!(format!("{:?}", goal), "ForAll<type> { Exists<type> { ForAll<type> { ?0: Foo<?1, ?2> } } }");
    });
}

#[test]
fn atc_accounting() {
    let program = Arc::new(parse_and_lower("
            struct Vec<T> { }

            trait Iterable {
                type Iter<'a>;
            }

            impl<T> Iterable for Vec<T> {
                type Iter<'a> = Iter<'a, T>;
            }

            struct Iter<'a, T> { }
    ").unwrap());
    set_current_program(&program, || {
        let impl_text = format!("{:#?}", &program.impl_data.values().next().unwrap());
        println!("{}", impl_text);
        assert_eq!(&impl_text[..], r#"ImplDatum {
    binders: for<type> ImplDatumBound {
        trait_ref: Positive(
            Vec<?0> as Iterable
        ),
        where_clauses: [],
        associated_ty_values: [
            AssociatedTyValue {
                associated_ty_id: (Iterable::Iter),
                value: for<lifetime> AssociatedTyValueBound {
                    ty: Iter<'?0, ?1>,
                    where_clauses: []
                }
            }
        ],
        specialization_priority: 0
    }
}"#);
        let goal = parse_and_lower_goal(&program, "forall<X> { forall<'a> { forall<Y> { \
                                                   X: Iterable<Iter<'a> = Y> } } }")
            .unwrap();
        let goal_text = format!("{:?}", goal);
        println!("{}", goal_text);
        assert_eq!(goal_text, "ForAll<type> { ForAll<lifetime> { ForAll<type> { <?2 as Iterable>::Iter<'?1> ==> ?0 } } }");
    });
}

#[test]
fn check_parameter_kinds() {

    lowering_error! {
        program {
            struct Foo<'a> { }
            struct i32 { }
            trait Bar { }
            impl Bar for Foo<i32> { }
        }
        error_msg {
            "incorrect parameter kind: expected lifetime, found type"
        }
    };

    lowering_error! {
        program {
            struct Foo<T> { }
            trait Bar { }
            impl<'a> Bar for Foo<'a> { }
        }
        error_msg {
            "incorrect parameter kind: expected type, found lifetime"
        }
    };

    lowering_error! {
        program {
            trait Iterator { type Item<'a>; }
            trait Foo { }
            impl<X, T> Foo for <X as Iterator>::Item<T> where X: Iterator { }
        }
        error_msg {
            "incorrect kind for associated type parameter: expected lifetime, found type"
        }
    };

    lowering_error! {
        program {
            trait Iterator { type Item<T>; }
            trait Foo { }
            impl<X, 'a> Foo for <X as Iterator>::Item<'a> where X: Iterator { }
        }
        error_msg {
            "incorrect kind for associated type parameter: expected type, found lifetime"
        }
    };

    lowering_error! {
        program {
            trait Into<T> {}
            struct Foo {}
            impl<'a> Into<'a> for Foo {}
        }
        error_msg {
            "incorrect kind for trait parameter: expected type, found lifetime"
        }
    }

    lowering_error! {
        program {
            trait IntoTime<'a> {}
            struct Foo {}
            impl<T> IntoTime<T> for Foo {}
        }
        error_msg {
            "incorrect kind for trait parameter: expected lifetime, found type"
        }
    }
}

#[test]
fn two_impls_for_same_type() {
    lowering_error! {
        program {
            trait Foo { }
            struct Bar { }
            impl Foo for Bar { }
            impl Foo for Bar { }
        }
        error_msg {
            "overlapping impls of trait \"Foo\""
        }
    }
}

#[test]
fn generic_vec_and_specific_vec() {
    lowering_success! {
        program {
            trait Foo { }
            struct Vec<T> { }
            struct Bar { }
            impl Foo for Vec<Bar> { }
            impl<T> Foo for Vec<T> { }
        }
    }
}

#[test]
fn concrete_impl_and_blanket_impl() {
    lowering_success! {
        program {
            trait Foo { }
            struct Bar { }
            impl Foo for Bar { }
            impl<T> Foo for T { }
        }
    }
}

#[test]
fn two_blanket_impls() {
    lowering_error! {
        program {
            trait Foo { }
            trait Bar { }
            trait Baz { }
            impl<T> Foo for T where T: Bar { }
            impl<T> Foo for T where T: Baz { }
            struct Quux { }
            impl Bar for Quux { }
            impl Baz for Quux { }
        }
        error_msg {
            "overlapping impls of trait \"Foo\""
        }
    }
}

#[test]
// FIXME This should be an error
// We currently assume a closed universe always, but overlaps checking should
// assume an open universe - what if a client implemented both Bar and Baz
//
// In other words, this should have the same behavior as the two_blanket_impls
// test.
fn two_blanket_impls_open_ended() {
    lowering_success! {
        program {
            trait Foo { }
            trait Bar { }
            trait Baz { }
            impl<T> Foo for T where T: Bar { }
            impl<T> Foo for T where T: Baz { }
        }
    }
}

#[test]
fn multiple_nonoverlapping_impls() {
    lowering_success! {
        program {
            trait Foo { }
            struct Bar { }
            struct Baz<T> { }
            impl Foo for Bar { }
            impl<T> Foo for Baz<T> { }
        }
    }
}

#[test]
fn local_negative_reasoning_in_coherence() {
    lowering_success! {
        program {
            trait Foo { }
            trait Bar { }
            struct Baz { }
            impl<T> Foo for T where T: Bar { }
            impl Foo for Baz { }
        }
    }
}

#[test]
fn multiple_parameters() {
    lowering_error! {
        program {
            trait Foo<T> { }
            struct Baz { }

            impl<T> Foo<Baz> for T { }
            impl<T> Foo<T> for Baz { }
        } error_msg {
            "overlapping impls of trait \"Foo\""
        }
    }
}

#[test]
fn nonoverlapping_assoc_types() {
    lowering_success! {
        program {
            trait Iterator {
                type Item;
            }
            struct Bar { }
            impl Iterator for Bar {
                type Item = Bar;
            }
            struct Baz<T> { }
            impl<T> Iterator for Baz<T> {
                type Item = Baz<T>;
            }

            trait Foo { }
            impl Foo for <Bar as Iterator>::Item { }
            impl<T> Foo for <Baz<T> as Iterator>::Item { }
        }
    }
}

#[test]
fn overlapping_assoc_types() {
    lowering_error! {
        program {
            trait Foo<T> { }

            trait Iterator { type Item; }


            struct Vec<T> { }
            impl<T> Iterator for Vec<T> { type Item = T; }

            impl<T> Foo<<T as Iterator>::Item> for T where T: Iterator { }

            impl<A, B> Foo<A> for B { }
        } error_msg {
            "overlapping impls of trait \"Foo\""
        }
    }
}

#[test]
fn overlapping_negative_positive_impls() {
    lowering_error! {
        program {
            trait Send { }
            struct i32 { }

            impl Send for i32 { }
            impl !Send for i32 { }
        } error_msg {
            "overlapping impls of trait \"Send\""
        }
    }
}

#[test]
fn overlapping_negative_impls() {
    lowering_success! {
        program {
            trait Send { }
            trait Foo { }
            trait Bar { }

            struct Vec<T> { }
            struct i32 { }

            impl Foo for i32 { }
            impl Bar for i32 { }

            impl<T> !Send for Vec<T> where T: Foo { }
            impl<T> !Send for Vec<T> where T: Bar { }
        }
    }
}
