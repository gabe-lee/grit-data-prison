#![allow(unused)]

use grit_data_prison::{AccessError};
use grit_data_prison::single_threaded::Prison;

#[derive(PartialEq, Debug)]
enum Var {
    F64(f64),
    Null
}

#[test]
fn prison_returns_the_correct_index_for_inserted_values() {
    let mut prison = Prison::new();
    let val_1 = Var::F64(3.1415*1.0);
    let val_2 = Var::F64(3.1415*2.0);
    let val_3 = Var::F64(3.1415*3.0);
    let idx_1 = prison.push(val_1);
    let idx_2 = prison.push(val_2);
    let idx_3 = prison.push(val_3);
    assert_eq!(idx_1.unwrap(), 0);
    assert_eq!(idx_2.unwrap(), 1);
    assert_eq!(idx_3.unwrap(), 2);
    prison.visit(0, |val| {
        assert_eq!(*val, Var::F64(3.1415*1.0), "Prison either returned the wrong index for inserted value or changed value on insertion");
    });
    prison.visit(1, |val| {
        assert_eq!(*val, Var::F64(3.1415*2.0), "Prison either returned the wrong index for inserted value or changed value on insertion");
    });
    prison.visit(2, |val| {
        assert_eq!(*val, Var::F64(3.1415*3.0), "Prison either returned the wrong index for inserted value or changed value on insertion");
    });
}

#[test]
fn prison_errors_when_visiting_index_out_of_range() {
    let mut prison = Prison::new();
    prison.push(Var::F64(1.0));
    assert!(prison.visit(0, |val| {}).is_ok(), "Prison could not access valid index");
    assert!(prison.visit(1, |val| {}).is_err(), "Prison allowed access to invalid index");
}

#[test]
fn prison_can_access_same_cell_consecutively() {
    let mut prison = Prison::new();
    prison.push(Var::F64(1.0));
    assert!(prison.visit(0, |val| {}).is_ok(), "Prison could not access newly created cell");
    assert!(prison.visit(0, |val| {}).is_ok(), "Prison could not access cell a second time");
}

#[test]
fn prison_errors_when_visiting_same_cell_simultaneously() {
    let mut prison = Prison::new();
    prison.push(Var::F64(1.0));
    prison.visit(0, |val| {
        let result = prison.visit(0, |same_val| {});
        assert!(result.is_err(), "Prison allowed simultaneous visits to same cell");
    });
}

#[test]
fn prison_can_access_different_cells_simultaneously() {
    let mut prison = Prison::new();
    prison.push(Var::F64(1.0));
    prison.push(Var::Null);
    assert!(prison.visit(0, |val| {
        assert!(prison.visit(1, |val| {}).is_ok(), "Prison could not visit second cell while visiting first cell");
    }).is_ok(), "Prison could not access first cell");
}

#[test]
fn pushing_to_prison_below_max_capacity_while_visiting_is_fine() {
    let mut prison = Prison::with_capacity(5);
    prison.push(Var::F64(3.1415));
    assert!(prison.visit(0, |val| {
        assert_eq!(*val, Var::F64(3.1415), "Value inserted into prison did not match value during visit");
        prison.push(Var::Null);
        prison.push(Var::Null);
        prison.push(Var::Null);
        prison.push(Var::Null);
    }).is_ok(), "Prison could not access first cell");
}

#[test]
fn pushing_to_prison_at_max_capacity_while_not_visiting_is_fine() {
    let mut prison = Prison::with_capacity(5);
    prison.push(Var::F64(3.1415));
    assert!(prison.visit(0, |val| {
        assert_eq!(*val, Var::F64(3.1415), "Value inserted into prison did not match value during visit");
        assert!(prison.push(Var::Null).is_ok());
        assert!(prison.push(Var::Null).is_ok());
        assert!(prison.push(Var::Null).is_ok());
        assert!(prison.push(Var::Null).is_ok());
    }).is_ok(), "Prison could not access first cell");
    assert!(prison.push(Var::F64(9.9999)).is_ok());
    assert!(prison.visit(5, |val_5| {}).is_ok(), "Could not visit prison after resize");
}

#[test]
fn pushing_to_prison_at_max_capacity_while_visiting_errors() {
    let mut prison = Prison::with_capacity(5);
    prison.push(Var::F64(3.1415));
    assert!(prison.visit(0, |val| {
        assert_eq!(*val, Var::F64(3.1415), "Value inserted into prison did not match value during visit");
        assert!(prison.push(Var::Null).is_ok());
        assert!(prison.push(Var::Null).is_ok());
        assert!(prison.push(Var::Null).is_ok());
        assert!(prison.push(Var::Null).is_ok());
        assert!(prison.push(Var::Null).is_err(), "Prison allowed push while at max capaity AND visiting");
    }).is_ok(), "Prison could not access first cell");
}

#[test]
fn prison_pop_method_errors_only_if_last_value_being_visited() {
    let mut prison = Prison::new();
    prison.push(Var::F64(1.0));
    prison.push(Var::F64(2.0));
    prison.push(Var::F64(3.0));
    assert!(prison.visit(2, |val| {
        // let new_idx = prison.push(Var::F64(9.9999));
        // assert_eq!(new_idx, 1, "Prison did not return correct index for pushed value");
        assert!(prison.pop().is_err(), "Prison allowed pop() while last index was visited");
    }).is_ok(), "Prison could not access last cell");
    assert!(prison.visit(1, |val| {
        assert!(prison.pop().is_ok(), "Prison did not allow pop() while second-to-last index was visited");
    }).is_ok(), "Prison could not access second-to-last cell");
}

#[test]
fn prison_visit_many_allows_access_to_disjoint_cells() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(1.0));
    prison.push(Var::F64(2.0));
    prison.push(Var::F64(3.0));
    prison.push(Var::F64(4.0));
    prison.push(Var::F64(5.0));
    assert!(prison.visit_many(&[0,2,4], |val_set_1| {
        assert!(prison.visit_many(&[1,3,5], |val_set_2| {
            assert_eq!(*val_set_1[0], Var::F64(0.0), "First val in first set did not match original value");
            assert_eq!(*val_set_1[1], Var::F64(2.0), "Second val in first set did not match original value");
            assert_eq!(*val_set_1[2], Var::F64(4.0), "Third val in first set did not match original value");
            assert_eq!(*val_set_2[0], Var::F64(1.0), "First val in second set did not match original value");
            assert_eq!(*val_set_2[1], Var::F64(3.0), "Second val in second set did not match original value");
            assert_eq!(*val_set_2[2], Var::F64(5.0), "Third val in second set did not match original value");
        }).is_ok(), "Prison could not access second set");
    }).is_ok(), "Prison could not access first set");
}

#[test]
fn prison_visit_many_errors_on_index_out_of_ranget() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(1.0));
    prison.push(Var::F64(2.0));
    prison.push(Var::F64(3.0));
    prison.push(Var::F64(4.0));
    prison.push(Var::F64(5.0));
    assert!(prison.visit_many(&[0,1,2,3,4,5,6], |val_set_1| {
    }).is_err(), "Prison allowed visit to index out of range");
}

#[test]
fn prison_visit_many_errors_on_double_visit() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(1.0));
    prison.push(Var::F64(2.0));
    prison.push(Var::F64(3.0));
    prison.push(Var::F64(4.0));
    prison.push(Var::F64(5.0));
    assert!(prison.visit_many(&[0,1,2,3,4,5,5], |val_set_1| {
    }).is_err(), "Prison allowed double visit when given duplicate indexes in same visit");
    prison.visit_many(&[0,1,2,3], |val_set_1| {
        assert!(prison.visit_many(&[3,4,5], |val_set_2| {
        }).is_err(), "Prison allowed double visit when given duplicate simultaneous visit_many");
        assert!(prison.visit(1, |val| {
        }).is_err(), "Prison allowed double visit of cell");
    });
}

#[test]
fn prison_visit_many_cleans_up_locks_on_failed_and_good_visits() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(1.0));
    prison.push(Var::F64(2.0));
    prison.push(Var::F64(3.0));
    prison.push(Var::F64(4.0));
    prison.push(Var::F64(5.0));
    prison.visit_many(&[0,1,2,3,4,5,5], |val_set_1| {
    });
    assert!(prison.visit_many(&[0,1,2,3,4,5], |val_set_1| {
    }).is_ok(), "Prison.visit_many() did not clean up all locks it took out on failed visit");
    assert!(prison.visit_many(&[0,1,2,3,4,5], |val_set_1| {
    }).is_ok(), "Prison.visit_many() did not clean up all locks it took out on good visit");
}

#[test]
fn prison_visit_slice_works_identical_to_consecutive_visit_many() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(1.0));
    prison.push(Var::F64(2.0));
    prison.push(Var::F64(3.0));
    prison.push(Var::F64(4.0));
    prison.push(Var::F64(5.0));
    prison.visit_many(&[0,1,2,3,4,5], |val_set_1| {
        *val_set_1[0] = Var::F64(10.0);
        *val_set_1[1] = Var::F64(11.0);
        *val_set_1[2] = Var::F64(12.0);
        *val_set_1[3] = Var::F64(13.0);
        *val_set_1[4] = Var::F64(14.0);
        *val_set_1[5] = Var::F64(15.0);
    });
    assert!(prison.visit_slice(0..6, |val_slice| {
        assert_eq!(*val_slice[0], Var::F64(10.0));
        assert_eq!(*val_slice[1], Var::F64(11.0));
        assert_eq!(*val_slice[2], Var::F64(12.0));
        assert_eq!(*val_slice[3], Var::F64(13.0));
        assert_eq!(*val_slice[4], Var::F64(14.0));
        assert_eq!(*val_slice[5], Var::F64(15.0));
    }).is_ok(), "Prison.visit_slice() could not access valid indexes");
}

#[test]
fn prison_visit_each_correctly_visits_every_cell() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    assert!(prison.visit_each(|idx, val| {
        *val = Var::F64(1.0)
    }).is_ok(), "Prison.visit_each() failed");
    prison.visit_slice(0..6, |val_slice| {
        assert_eq!(*val_slice[0], Var::F64(1.0));
        assert_eq!(*val_slice[1], Var::F64(1.0));
        assert_eq!(*val_slice[2], Var::F64(1.0));
        assert_eq!(*val_slice[3], Var::F64(1.0));
        assert_eq!(*val_slice[4], Var::F64(1.0));
        assert_eq!(*val_slice[5], Var::F64(1.0));
    });
}

#[test]
fn prison_visit_each_can_visit_other_free_cells() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    assert!(prison.visit_each(|idx, val| {
        if idx > 0 {
            assert!(prison.visit(idx-1, |last_val| {
                *last_val = Var::F64(2.0)
            }).is_ok(), "Prison.visit_each() could not visit a free cell from within its own visit");
        }
        if idx < prison.len()-1 {
            assert!(prison.visit(idx+1, |last_val| {
                *last_val = Var::F64(2.0)
            }).is_ok(), "Prison.visit_each() could not visit a free cell from within its own visit");
        }
        *val = Var::F64(1.0)
    }).is_ok(), "Prison.visit_each() failed");
    prison.visit_slice(0..6, |val_slice| {
        assert_eq!(*val_slice[0], Var::F64(2.0));
        assert_eq!(*val_slice[1], Var::F64(2.0));
        assert_eq!(*val_slice[2], Var::F64(2.0));
        assert_eq!(*val_slice[3], Var::F64(2.0));
        assert_eq!(*val_slice[4], Var::F64(2.0));
        assert_eq!(*val_slice[5], Var::F64(1.0));
    });
}

#[test]
fn prison_visit_each_in_range_errors_on_invalid_range() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    assert!(prison.visit_each_in_range(0..10, |idx, val| {}).is_err(), "Prison.visit_each_in_range() allowed illegal range");
    assert!(prison.visit_each_in_range(10..100, |idx, val| {}).is_err(), "Prison.visit_each_in_range() allowed illegal range");
    assert!(prison.visit_each_in_range(10..0, |idx, val| {}).is_err(), "Prison.visit_each_in_range() allowed illegal range");
}

#[test]
fn prison_visit_slice_errors_on_invalid_range() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    assert!(prison.visit_slice(0..10, |val| {}).is_err(), "Prison.visit_slice() allowed illegal range");
    assert!(prison.visit_slice(10..100, |val| {}).is_err(), "Prison.visit_slice() allowed illegal range");
}

#[test]
fn prison_visit_each_in_range_supplies_correct_index_for_each_run() {
    let mut prison = Prison::new();
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    prison.push(Var::F64(0.0));
    let mut should_be_idx: usize = 0;
    prison.visit_each_in_range(0..=5, |idx, val| {
        assert_eq!(should_be_idx, idx, "Prison.visit_each_in_range() supplied the wrong index");
        should_be_idx += 1;
    });
}