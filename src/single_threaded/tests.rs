#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
//====== Testing ======
use std::{fmt::Display, mem};

use super::*;

//MACRO assert_prison_state!
/// prison, access_count, gen, next_free, free_count, vec_len
macro_rules! assert_prison_state {
    ($P:ident, $A_CNT:expr, $GEN:expr, $NEXT:expr, $F_CNT:expr, $LEN:expr) => {
        let p = &internal!($P);
        if p.access_count != $A_CNT
            || p.generation != $GEN
            || p.next_free != $NEXT
            || p.free_count != $F_CNT
            || p.vec.len() != $LEN {
                panic!("\nIncorrect prison state:\n\tEXP:\taccess_count: {}, gen: {}, next_free: {}, free_count: {}, vec_len: {}\n\tGOT:\taccess_count: {}, gen: {}, next_free: {}, free_count: {}, vec_len: {}\n",
                $A_CNT, $GEN, $NEXT, $F_CNT, $LEN,
                p.access_count, p.generation, p.next_free, p.free_count, p.vec.len());
            }
    };
}

//MACRO assert_cell_state!
/// prison, index, refs, gen, val
macro_rules! assert_cell_state {
    ($P:ident, $IDX:expr, $REFS:expr, $GEN:expr, $VAL:expr) => {
        match &internal!($P).vec[$IDX] {
            cell if (cell.is_cell() && cell.refs_or_next == $REFS && IdxD::val(cell.d_gen_or_prev) == $GEN && unsafe {cell.val.assume_init_ref()} == &$VAL) => {},
            cell if cell.is_cell() => panic!("\nIndex {} unexpected state:\n\tEXP:\trefs = {}, gen = {}, val = {}\n\tGOT:\trefs = {}, gen = {}, val = {}\n", $IDX, $REFS, $GEN, $VAL, cell.refs_or_next, IdxD::val(cell.d_gen_or_prev), unsafe {cell.val.assume_init_ref()}),
            _ => panic!("\nIndex {} wrong variant:\n\tEXP:\t`Cell`\n\tGOT:\t`Free`\n", $IDX)
        }
    };
}

//MACRO assert_free_state!
/// prison, index, prev_free, next_free
macro_rules! assert_free_state {
    ($P:ident, $IDX:expr, $PREV:expr, $NEXT:expr) => {
        match &internal!($P).vec[$IDX] {
            free if (free.is_free() && free.refs_or_next == $NEXT && IdxD::val(free.d_gen_or_prev) == $PREV) => {},
            free if free.is_free() => panic!("\nIndex {} unexpected state:\n\tEXP:\tprev_free = {}, next_free = {}\n\tGOT:\tprev_free = {}, next_free = {}\n", $IDX, $PREV, $NEXT, IdxD::val(free.d_gen_or_prev), free.refs_or_next),
            _ => panic!("\nIndex {} wrong variant:\n\tEXP:\t`Free`\n\tGOT:\t`Cell`\n", $IDX)
        }
    };
}

//MACRO assert_jail_state!
/// jail, refs, val
macro_rules! assert_jail_state {
    ($J:ident, $REFS:expr, $VAL:expr) => {
        match &internal!($J) {
            jail if (jail.refs == $REFS && jail.val == $VAL) => {},
            jail => panic!("\nJailCell unexpected state:\n\tEXP:\trefs = {}, val = {}\n\tGOT:\trefs = {}, val = {}\n", $REFS, $VAL, jail.refs, jail.val),
        }
    };
}

//MACRO assert_access_err!
/// operation, error
macro_rules! assert_access_err {
    ($OP:expr, $ERR:expr) => {
        match $OP {
            Err(e) if (e == $ERR) => {}
            Err(e) => panic!(
                "\nOperation returned incorrect error:\n\tEXP:\t{}\n\tGOT:\t{}\n",
                $ERR.kind(),
                e.kind()
            ),
            _ => panic!(
                "\nOperation failed to return error:\n\tEXP:\tErr({})\n\tGOT:\tOk(*)\n",
                $ERR.kind()
            ),
        }
    };
}

//MACRO assert_cell_key!
/// operation, index, gen
macro_rules! assert_cell_key {
    ($OP:expr, $IDX:expr, $GEN:expr) => {
        match $OP {
            Ok(key) if (key.idx == $IDX && key.gen == $GEN) => key,
            Ok(key) => panic!("\nOperation returned incorrect CellKey:\n\tEXP:\tidx = {}, gen = {}\n\tGOT:\tidx = {}, gen = {}\n", $IDX, $GEN, key.idx, key.gen),
            Err(e) => panic!("\nOperation failed to return CellKey:\n\tEXP:\tCellKey{{ idx: {}, gen: {}}}\n\tGOT:\tErr({})\n", $IDX, $GEN, e.kind())
        }
    };
}

#[derive(Debug, Eq, PartialEq)]
struct MyNoCopy(usize);

impl Display for MyNoCopy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// impl MyNoCopy {
//     fn val(&self) -> usize {
//         self.0
//     }
// }

// fn extract_usize(mnc: &MyNoCopy) -> usize {
//     mnc.val()
// }

//TEST memory footprint
#[test]
#[ignore]
fn memory_footprint() -> Result<(), AccessError> {
    // Prison
    assert_eq!(mem::size_of::<PrisonCell<()>>(), 16);
    assert_eq!(mem::size_of::<PrisonCell<u8>>(), 24);
    assert_eq!(mem::size_of::<PrisonCell<u64>>(), 24);
    assert_eq!(
        mem::size_of::<PrisonCell<(u8, u8, u8, u8, u8, u8, u8, u8, u8)>>(),
        32
    );
    assert_eq!(mem::size_of::<PrisonCell<u128>>(), 32);
    let vec_size = mem::size_of::<Vec<u8>>();
    assert_eq!(mem::size_of::<Prison<u8>>(), 32 + vec_size);
    // JailCell
    assert_eq!(mem::size_of::<JailCell<()>>(), 8);
    assert_eq!(mem::size_of::<JailCell<u8>>(), 16);
    assert_eq!(mem::size_of::<JailCell<u64>>(), 16);
    assert_eq!(
        mem::size_of::<JailCell<(u8, u8, u8, u8, u8, u8, u8, u8, u8)>>(),
        24
    );
    assert_eq!(mem::size_of::<JailCell<u128>>(), 24);
    Ok(())
}

//------ Prison tests ------
//TODO: TEST Prison::new()
//TODO: TEST Prison::with_capacity()
//TODO: TEST Prison::vec_len()
//TODO: TEST Prison::vec_cap()
//TODO: TEST Prison::num_free()
//TODO: TEST Prison::num_used()
//TODO: TEST Prison::density()

//TEST Prison::insert()
#[test]
fn prison_insert() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 0);
    let key_0 = assert_cell_key!(prison.insert(MyNoCopy(0)), 0, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 1);
    let key_1 = assert_cell_key!(prison.insert(MyNoCopy(1)), 1, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 2);
    let key_2 = assert_cell_key!(prison.insert(MyNoCopy(2)), 2, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 3);
    prison.visit_ref(key_0, |val_0| {
        assert_access_err!(
            prison.insert(MyNoCopy(3)),
            AccessError::InsertAtMaxCapacityWhileAValueIsReferenced
        );
        Ok(())
    })?;
    prison.visit_mut(key_0, |val_0| {
        assert_access_err!(
            prison.insert(MyNoCopy(3)),
            AccessError::InsertAtMaxCapacityWhileAValueIsReferenced
        );
        Ok(())
    })?;
    let key_3 = assert_cell_key!(prison.insert(MyNoCopy(3)), 3, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 4);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
    prison.remove(key_2)?;
    assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
    assert_prison_state!(prison, 0, 1, 2, 1, 4);
    let key_2 = assert_cell_key!(prison.insert(MyNoCopy(22)), 2, 1);
    assert_cell_state!(prison, 2, 0, 1, MyNoCopy(22));
    assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 4);
    Ok(())
}

//TEST Prison::insert_at()
#[test]
fn prison_insert_at() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 0);
    assert_access_err!(
        prison.insert_at(0, MyNoCopy(0)),
        AccessError::IndexOutOfRange(0)
    );
    let key_0 = assert_cell_key!(prison.insert(MyNoCopy(0)), 0, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 1);
    let key_1 = assert_cell_key!(prison.insert(MyNoCopy(1)), 1, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 2);
    let key_2 = assert_cell_key!(prison.insert(MyNoCopy(2)), 2, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 3);
    prison.remove(key_0)?;
    assert_prison_state!(prison, 0, 1, 0, 1, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, IdxD::INVALID);
    prison.remove(key_1)?;
    assert_prison_state!(prison, 0, 1, 1, 2, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    prison.remove(key_2)?;
    assert_free_state!(prison, 2, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 2, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    assert_prison_state!(prison, 0, 1, 2, 3, 3);
    let key_0 = assert_cell_key!(prison.insert_at(0, MyNoCopy(10)), 0, 1);
    assert_prison_state!(prison, 0, 1, 2, 2, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 2, IdxD::INVALID);
    let key_1 = assert_cell_key!(prison.insert_at(1, MyNoCopy(11)), 1, 1);
    assert_prison_state!(prison, 0, 1, 2, 1, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
    let key_2 = assert_cell_key!(prison.insert_at(2, MyNoCopy(12)), 2, 1);
    assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 3);
    assert_access_err!(
        prison.insert_at(0, MyNoCopy(0)),
        AccessError::IndexIsNotFree(0)
    );
    assert_cell_state!(prison, 0, 0, 1, MyNoCopy(10));
    assert_cell_state!(prison, 1, 0, 1, MyNoCopy(11));
    assert_cell_state!(prison, 2, 0, 1, MyNoCopy(12));
    prison.remove(key_0)?;
    prison.remove(key_1)?;
    prison.remove(key_2)?;
    let key_1 = assert_cell_key!(prison.insert_at(1, MyNoCopy(111)), 1, 2);
    assert_free_state!(prison, 2, IdxD::INVALID, 0);
    assert_cell_state!(prison, 1, 0, 2, MyNoCopy(111));
    assert_free_state!(prison, 0, 2, IdxD::INVALID);
    Ok(())
}

//TEST Prison::overwrite()
#[test]
fn prison_overwrite() -> Result<(), AccessError> {
    // test `overwrite()` behaves exactly like `insert_at()` when given a free index
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 0);
    assert_access_err!(
        prison.insert_at(0, MyNoCopy(0)),
        AccessError::IndexOutOfRange(0)
    );
    let key_0 = assert_cell_key!(prison.insert(MyNoCopy(0)), 0, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 1);
    let key_1 = assert_cell_key!(prison.insert(MyNoCopy(1)), 1, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 2);
    let key_2 = assert_cell_key!(prison.insert(MyNoCopy(2)), 2, 0);
    assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 3);
    prison.remove(key_0)?;
    assert_prison_state!(prison, 0, 1, 0, 1, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, IdxD::INVALID);
    prison.remove(key_1)?;
    assert_prison_state!(prison, 0, 1, 1, 2, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    prison.remove(key_2)?;
    assert_free_state!(prison, 2, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 2, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    assert_prison_state!(prison, 0, 1, 2, 3, 3);
    let key_0 = assert_cell_key!(prison.overwrite(0, MyNoCopy(10)), 0, 1);
    assert_prison_state!(prison, 0, 1, 2, 2, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 2, IdxD::INVALID);
    let key_1 = assert_cell_key!(prison.overwrite(1, MyNoCopy(11)), 1, 1);
    assert_prison_state!(prison, 0, 1, 2, 1, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
    let key_2 = assert_cell_key!(prison.overwrite(2, MyNoCopy(12)), 2, 1);
    assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 3);
    assert_cell_state!(prison, 0, 0, 1, MyNoCopy(10));
    assert_cell_state!(prison, 1, 0, 1, MyNoCopy(11));
    assert_cell_state!(prison, 2, 0, 1, MyNoCopy(12));
    prison.remove(key_0)?;
    prison.remove(key_1)?;
    prison.remove(key_2)?;
    let key_1 = assert_cell_key!(prison.overwrite(1, MyNoCopy(111)), 1, 2);
    assert_free_state!(prison, 2, IdxD::INVALID, 0);
    assert_cell_state!(prison, 1, 0, 2, MyNoCopy(111));
    assert_free_state!(prison, 0, 2, IdxD::INVALID);
    // Test overwriting filled cell
    let key_1 = assert_cell_key!(prison.overwrite(1, MyNoCopy(1111)), 1, 3);
    assert_cell_state!(prison, 1, 0, 3, MyNoCopy(1111));
    assert_prison_state!(prison, 0, 3, 2, 2, 3);
    Ok(())
}

//TEST Prison::remove()
#[test]
fn prison_remove() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    prison.remove(key_0)?;
    assert_prison_state!(prison, 0, 1, 0, 1, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, IdxD::INVALID);
    prison.remove(key_1)?;
    assert_prison_state!(prison, 0, 1, 1, 2, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    prison.remove(key_2)?;
    assert_free_state!(prison, 2, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 2, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    assert_prison_state!(prison, 0, 1, 2, 3, 3);
    let key_0 = prison.insert_at(0, MyNoCopy(10))?;
    let key_1 = prison.insert_at(1, MyNoCopy(11))?;
    let key_2 = prison.insert_at(2, MyNoCopy(12))?;
    assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 3);
    prison.remove(key_2)?;
    assert_prison_state!(prison, 0, 2, 2, 1, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
    prison.remove(key_1)?;
    assert_prison_state!(prison, 0, 2, 1, 2, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, 2);
    assert_free_state!(prison, 2, 1, IdxD::INVALID);
    prison.remove(key_0)?;
    assert_prison_state!(prison, 0, 2, 0, 3, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 0, 2);
    assert_free_state!(prison, 2, 1, IdxD::INVALID);
    let key_0 = prison.insert_at(0, MyNoCopy(100))?;
    let key_1 = prison.insert_at(1, MyNoCopy(110))?;
    let key_2 = prison.insert_at(2, MyNoCopy(120))?;
    assert_prison_state!(prison, 0, 2, IdxD::INVALID, 0, 3);
    prison.remove(key_1)?;
    assert_prison_state!(prison, 0, 3, 1, 1, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, IdxD::INVALID);
    prison.remove(key_0)?;
    assert_prison_state!(prison, 0, 3, 0, 2, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 0, IdxD::INVALID);
    prison.remove(key_2)?;
    assert_prison_state!(prison, 0, 3, 2, 3, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, 0);
    assert_free_state!(prison, 0, 2, 1);
    assert_free_state!(prison, 1, 0, IdxD::INVALID);
    Ok(())
}

//TEST Prison::remove_idx()
#[test]
fn prison_remove_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.remove_idx(0)?;
    assert_prison_state!(prison, 0, 1, 0, 1, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, IdxD::INVALID);
    prison.remove_idx(1)?;
    assert_prison_state!(prison, 0, 1, 1, 2, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    prison.remove_idx(2)?;
    assert_free_state!(prison, 2, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 2, 0);
    assert_free_state!(prison, 0, 1, IdxD::INVALID);
    assert_prison_state!(prison, 0, 1, 2, 3, 3);
    prison.insert_at(0, MyNoCopy(10))?;
    prison.insert_at(1, MyNoCopy(11))?;
    prison.insert_at(2, MyNoCopy(12))?;
    assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 3);
    prison.remove_idx(2)?;
    assert_prison_state!(prison, 0, 2, 2, 1, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
    prison.remove_idx(1)?;
    assert_prison_state!(prison, 0, 2, 1, 2, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, 2);
    assert_free_state!(prison, 2, 1, IdxD::INVALID);
    prison.remove_idx(0)?;
    assert_prison_state!(prison, 0, 2, 0, 3, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 0, 2);
    assert_free_state!(prison, 2, 1, IdxD::INVALID);
    prison.insert_at(0, MyNoCopy(100))?;
    prison.insert_at(1, MyNoCopy(110))?;
    prison.insert_at(2, MyNoCopy(120))?;
    assert_prison_state!(prison, 0, 2, IdxD::INVALID, 0, 3);
    prison.remove_idx(1)?;
    assert_prison_state!(prison, 0, 3, 1, 1, 3);
    assert_free_state!(prison, 1, IdxD::INVALID, IdxD::INVALID);
    prison.remove_idx(0)?;
    assert_prison_state!(prison, 0, 3, 0, 2, 3);
    assert_free_state!(prison, 0, IdxD::INVALID, 1);
    assert_free_state!(prison, 1, 0, IdxD::INVALID);
    prison.remove_idx(2)?;
    assert_prison_state!(prison, 0, 3, 2, 3, 3);
    assert_free_state!(prison, 2, IdxD::INVALID, 0);
    assert_free_state!(prison, 0, 2, 1);
    assert_free_state!(prison, 1, 0, IdxD::INVALID);
    Ok(())
}

//TEST Prison::visit_mut()
#[test]
fn prison_visit_mut() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(
        prison.visit_mut(CellKey::from_raw_parts(0, 0), |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    prison.visit_mut(key_0, |val_0| {
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
        *val_0 = MyNoCopy(10);
        assert_eq!(*val_0, MyNoCopy(10));
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_access_err!(
            prison.visit_mut(key_0, |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_ref(key_0, |val_0| {
        assert_access_err!(
            prison.visit_mut(key_0, |_| Ok(())),
            AccessError::ValueStillImmutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_mut(key_0, |val_0| {
        prison.visit_mut(key_1, |val_1| {
            prison.visit_mut(key_2, |val_2| {
                assert_eq!(*val_0, MyNoCopy(10));
                assert_eq!(*val_1, MyNoCopy(1));
                assert_eq!(*val_2, MyNoCopy(2));
                *val_0 = MyNoCopy(100);
                *val_1 = MyNoCopy(200);
                *val_2 = MyNoCopy(300);
                assert_eq!(*val_0, MyNoCopy(100));
                assert_eq!(*val_1, MyNoCopy(200));
                assert_eq!(*val_2, MyNoCopy(300));
                assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
                assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
                assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
                Ok(())
            })?;
            Ok(())
        })?;
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    prison.remove(key_0)?;
    assert_access_err!(
        prison.visit_mut(key_0, |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    Ok(())
}

//TEST Prison::visit_ref()
#[test]
fn prison_visit_ref() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(
        prison.visit_ref(CellKey::from_raw_parts(0, 0), |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    prison.visit_ref(key_0, |val_0| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
        Ok(())
    })?;
    prison.visit_mut(key_0, |val_0| {
        assert_access_err!(
            prison.visit_ref(key_0, |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_ref(key_0, |val_0| {
        prison.visit_ref(key_1, |val_1| {
            prison.visit_ref(key_2, |val_2| {
                assert_eq!(*val_0, MyNoCopy(0));
                assert_eq!(*val_1, MyNoCopy(1));
                assert_eq!(*val_2, MyNoCopy(2));
                assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
                assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
                assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
                prison.visit_ref(key_2, |val_2_b| {
                    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
                    prison.visit_ref(key_2, |val_2_b| {
                        assert_cell_state!(prison, 2, 3, 0, MyNoCopy(2));
                        Ok(())
                    })?;
                    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
                    Ok(())
                })?;
                Ok(())
            })?;
            Ok(())
        })?;
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    prison.remove(key_0)?;
    assert_access_err!(
        prison.visit_ref(key_0, |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.visit_ref(key_1, |_| Ok(())),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::visit_mut_idx()
#[test]
fn prison_visit_mut_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(
        prison.visit_mut_idx(0, |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.visit_mut_idx(0, |val_0| {
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
        *val_0 = MyNoCopy(10);
        assert_eq!(*val_0, MyNoCopy(10));
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_access_err!(
            prison.visit_mut_idx(0, |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_ref_idx(0, |val_0| {
        assert_access_err!(
            prison.visit_mut_idx(0, |_| Ok(())),
            AccessError::ValueStillImmutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_mut_idx(0, |val_0| {
        prison.visit_mut_idx(1, |val_1| {
            prison.visit_mut_idx(2, |val_2| {
                assert_eq!(*val_0, MyNoCopy(10));
                assert_eq!(*val_1, MyNoCopy(1));
                assert_eq!(*val_2, MyNoCopy(2));
                *val_0 = MyNoCopy(100);
                *val_1 = MyNoCopy(200);
                *val_2 = MyNoCopy(300);
                assert_eq!(*val_0, MyNoCopy(100));
                assert_eq!(*val_1, MyNoCopy(200));
                assert_eq!(*val_2, MyNoCopy(300));
                assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
                assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
                assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
                Ok(())
            })?;
            Ok(())
        })?;
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.visit_mut_idx(0, |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    Ok(())
}

//TEST Prison::visit_ref_idx()
#[test]
fn prison_visit_ref_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(
        prison.visit_ref(CellKey::from_raw_parts(0, 0), |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.visit_ref_idx(0, |val_0| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
        Ok(())
    })?;
    prison.visit_mut_idx(0, |val_0| {
        assert_access_err!(
            prison.visit_ref_idx(0, |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_ref_idx(0, |val_0| {
        prison.visit_ref_idx(1, |val_1| {
            prison.visit_ref_idx(2, |val_2| {
                assert_eq!(*val_0, MyNoCopy(0));
                assert_eq!(*val_1, MyNoCopy(1));
                assert_eq!(*val_2, MyNoCopy(2));
                assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
                assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
                assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
                prison.visit_ref_idx(2, |val_2_b| {
                    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
                    prison.visit_ref_idx(2, |val_2_b| {
                        assert_cell_state!(prison, 2, 3, 0, MyNoCopy(2));
                        Ok(())
                    })?;
                    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
                    Ok(())
                })?;
                Ok(())
            })?;
            Ok(())
        })?;
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.visit_ref_idx(0, |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.visit_ref_idx(1, |_| Ok(())),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::visit_many_mut()
#[test]
fn prison_visit_many_mut() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.visit_many_mut(&[CellKey::from_raw_parts(0, 0)], |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.visit_many_mut(&[], |_| Ok(())).is_ok());
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    let key_3 = prison.insert(MyNoCopy(3))?;
    let key_4 = prison.insert(MyNoCopy(4))?;
    prison.visit_many_mut(&[key_0, key_1], |vals_0_1| {
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        *vals_0_1[0] = MyNoCopy(10);
        *vals_0_1[1] = MyNoCopy(11);
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_eq!(*vals_0_1[0], MyNoCopy(10));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(11));
        assert_eq!(*vals_0_1[1], MyNoCopy(11));
        assert_access_err!(
            prison.visit_many_mut(&[key_0], |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_ref(key_0, |val_0| {
        assert_access_err!(
            prison.visit_many_mut(&[key_0, key_1], |_| Ok(())),
            AccessError::ValueStillImmutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_many_mut(&[key_0, key_2, key_4], |vals_0_2_4| {
        prison.visit_many_mut(&[key_1, key_3], |vals_1_3| {
            assert_eq!(*vals_0_2_4[0], MyNoCopy(10));
            assert_eq!(*vals_1_3[0], MyNoCopy(11));
            assert_eq!(*vals_0_2_4[1], MyNoCopy(2));
            assert_eq!(*vals_1_3[1], MyNoCopy(3));
            assert_eq!(*vals_0_2_4[2], MyNoCopy(4));
            *vals_0_2_4[0] = MyNoCopy(100);
            *vals_1_3[0] = MyNoCopy(200);
            *vals_0_2_4[1] = MyNoCopy(300);
            *vals_1_3[1] = MyNoCopy(400);
            *vals_0_2_4[2] = MyNoCopy(500);
            assert_eq!(*vals_0_2_4[0], MyNoCopy(100));
            assert_eq!(*vals_1_3[0], MyNoCopy(200));
            assert_eq!(*vals_0_2_4[1], MyNoCopy(300));
            assert_eq!(*vals_1_3[1], MyNoCopy(400));
            assert_eq!(*vals_0_2_4[2], MyNoCopy(500));
            assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
            assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
            assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
            assert_cell_state!(prison, 3, Refs::MUT, 0, MyNoCopy(400));
            assert_cell_state!(prison, 4, Refs::MUT, 0, MyNoCopy(500));
            Ok(())
        })?;
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(500));
    prison.remove(key_0)?;
    assert_access_err!(
        prison.visit_many_mut(&[key_0], |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    Ok(())
}

//TEST Prison::visit_many_ref()
#[test]
fn prison_visit_many_ref() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.visit_many_ref(&[CellKey::from_raw_parts(0, 0)], |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.visit_many_ref(&[], |_| Ok(())).is_ok());
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    let key_3 = prison.insert(MyNoCopy(3))?;
    let key_4 = prison.insert(MyNoCopy(4))?;
    prison.visit_many_ref(&[key_0, key_1], |vals_0_1| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        Ok(())
    })?;
    prison.visit_mut(key_0, |val_0| {
        assert_access_err!(
            prison.visit_many_ref(&[key_0, key_1], |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_many_ref(&[key_0, key_1, key_2, key_3, key_4], |vals_a| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
        assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
        assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
        prison.visit_many_ref(&[key_0, key_1, key_2, key_3, key_4], |vals_b| {
            assert_eq!(*vals_a[0], MyNoCopy(0));
            assert_eq!(*vals_b[1], MyNoCopy(1));
            assert_eq!(*vals_a[2], MyNoCopy(2));
            assert_eq!(*vals_b[3], MyNoCopy(3));
            assert_eq!(*vals_a[4], MyNoCopy(4));
            assert_cell_state!(prison, 0, 2, 0, MyNoCopy(0));
            assert_cell_state!(prison, 1, 2, 0, MyNoCopy(1));
            assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
            assert_cell_state!(prison, 3, 2, 0, MyNoCopy(3));
            assert_cell_state!(prison, 4, 2, 0, MyNoCopy(4));
            Ok(())
        })?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
        assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
        assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(4));
    prison.remove(key_0)?;
    assert_access_err!(
        prison.visit_many_ref(&[key_0], |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.visit_many_ref(&[key_1], |_| Ok(())),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::visit_many_mut_idx()
#[test]
fn prison_visit_many_mut_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.visit_many_mut_idx(&[0], |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.visit_many_mut_idx(&[], |_| Ok(())).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    prison.visit_many_mut_idx(&[0, 1], |vals_0_1| {
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        *vals_0_1[0] = MyNoCopy(10);
        *vals_0_1[1] = MyNoCopy(11);
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_eq!(*vals_0_1[0], MyNoCopy(10));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(11));
        assert_eq!(*vals_0_1[1], MyNoCopy(11));
        assert_access_err!(
            prison.visit_many_mut_idx(&[0], |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_ref_idx(0, |val_0| {
        assert_access_err!(
            prison.visit_many_mut_idx(&[0, 1], |_| Ok(())),
            AccessError::ValueStillImmutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_many_mut_idx(&[0, 2, 4], |vals_0_2_4| {
        prison.visit_many_mut_idx(&[1, 3], |vals_1_3| {
            assert_eq!(*vals_0_2_4[0], MyNoCopy(10));
            assert_eq!(*vals_1_3[0], MyNoCopy(11));
            assert_eq!(*vals_0_2_4[1], MyNoCopy(2));
            assert_eq!(*vals_1_3[1], MyNoCopy(3));
            assert_eq!(*vals_0_2_4[2], MyNoCopy(4));
            *vals_0_2_4[0] = MyNoCopy(100);
            *vals_1_3[0] = MyNoCopy(200);
            *vals_0_2_4[1] = MyNoCopy(300);
            *vals_1_3[1] = MyNoCopy(400);
            *vals_0_2_4[2] = MyNoCopy(500);
            assert_eq!(*vals_0_2_4[0], MyNoCopy(100));
            assert_eq!(*vals_1_3[0], MyNoCopy(200));
            assert_eq!(*vals_0_2_4[1], MyNoCopy(300));
            assert_eq!(*vals_1_3[1], MyNoCopy(400));
            assert_eq!(*vals_0_2_4[2], MyNoCopy(500));
            assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
            assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
            assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
            assert_cell_state!(prison, 3, Refs::MUT, 0, MyNoCopy(400));
            assert_cell_state!(prison, 4, Refs::MUT, 0, MyNoCopy(500));
            Ok(())
        })?;
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(500));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.visit_many_mut_idx(&[0], |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    Ok(())
}

//TEST Prison::visit_many_ref_idx()
#[test]
fn prison_visit_many_ref_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.visit_many_ref_idx(&[0], |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.visit_many_ref_idx(&[], |_| Ok(())).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    prison.visit_many_ref_idx(&[0, 1], |vals_0_1| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        Ok(())
    })?;
    prison.visit_mut_idx(0, |val_0| {
        assert_access_err!(
            prison.visit_many_ref_idx(&[0, 1], |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_many_ref_idx(&[0, 1, 2, 3, 4], |vals_a| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
        assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
        assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
        prison.visit_many_ref_idx(&[0, 1, 2, 3, 4], |vals_b| {
            assert_eq!(*vals_a[0], MyNoCopy(0));
            assert_eq!(*vals_b[1], MyNoCopy(1));
            assert_eq!(*vals_a[2], MyNoCopy(2));
            assert_eq!(*vals_b[3], MyNoCopy(3));
            assert_eq!(*vals_a[4], MyNoCopy(4));
            assert_cell_state!(prison, 0, 2, 0, MyNoCopy(0));
            assert_cell_state!(prison, 1, 2, 0, MyNoCopy(1));
            assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
            assert_cell_state!(prison, 3, 2, 0, MyNoCopy(3));
            assert_cell_state!(prison, 4, 2, 0, MyNoCopy(4));
            Ok(())
        })?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
        assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
        assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(4));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.visit_many_ref_idx(&[0], |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.visit_many_ref_idx(&[1], |_| Ok(())),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::visit_slice_mut()
#[test]
fn prison_visit_slice_mut() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.visit_slice_mut(0..1, |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.visit_slice_mut(.., |_| Ok(())).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    prison.visit_slice_mut(0..=1, |vals_0_1| {
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        *vals_0_1[0] = MyNoCopy(10);
        *vals_0_1[1] = MyNoCopy(11);
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_eq!(*vals_0_1[0], MyNoCopy(10));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(11));
        assert_eq!(*vals_0_1[1], MyNoCopy(11));
        assert_access_err!(
            prison.visit_slice_mut(0..1, |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_ref_idx(0, |val_0| {
        assert_access_err!(
            prison.visit_slice_mut(0..1, |_| Ok(())),
            AccessError::ValueStillImmutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_slice_mut(..3, |vals_0_1_2| {
        prison.visit_slice_mut(3.., |vals_3_4| {
            assert_eq!(*vals_0_1_2[0], MyNoCopy(10));
            assert_eq!(*vals_0_1_2[1], MyNoCopy(11));
            assert_eq!(*vals_0_1_2[2], MyNoCopy(2));
            assert_eq!(*vals_3_4[0], MyNoCopy(3));
            assert_eq!(*vals_3_4[1], MyNoCopy(4));
            *vals_0_1_2[0] = MyNoCopy(100);
            *vals_0_1_2[1] = MyNoCopy(200);
            *vals_0_1_2[2] = MyNoCopy(300);
            *vals_3_4[0] = MyNoCopy(400);
            *vals_3_4[1] = MyNoCopy(500);
            assert_eq!(*vals_0_1_2[0], MyNoCopy(100));
            assert_eq!(*vals_0_1_2[1], MyNoCopy(200));
            assert_eq!(*vals_0_1_2[2], MyNoCopy(300));
            assert_eq!(*vals_3_4[0], MyNoCopy(400));
            assert_eq!(*vals_3_4[1], MyNoCopy(500));
            assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
            assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
            assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
            assert_cell_state!(prison, 3, Refs::MUT, 0, MyNoCopy(400));
            assert_cell_state!(prison, 4, Refs::MUT, 0, MyNoCopy(500));
            Ok(())
        })?;
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(500));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.visit_slice_mut(.., |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    Ok(())
}

//TEST Prison::visit_slice_ref()
#[test]
fn prison_visit_slice_ref() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.visit_slice_ref(0..1, |_| Ok(())),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.visit_slice_ref(.., |_| Ok(())).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    prison.visit_slice_ref(0..=1, |vals_0_1| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        Ok(())
    })?;
    prison.visit_mut_idx(0, |val_0| {
        assert_access_err!(
            prison.visit_slice_ref(0..1, |_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    prison.visit_slice_ref(.., |vals_a| {
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
        assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
        assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
        prison.visit_slice_ref(0..5, |vals_b| {
            assert_eq!(*vals_a[0], MyNoCopy(0));
            assert_eq!(*vals_b[1], MyNoCopy(1));
            assert_eq!(*vals_a[2], MyNoCopy(2));
            assert_eq!(*vals_b[3], MyNoCopy(3));
            assert_eq!(*vals_a[4], MyNoCopy(4));
            assert_cell_state!(prison, 0, 2, 0, MyNoCopy(0));
            assert_cell_state!(prison, 1, 2, 0, MyNoCopy(1));
            assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
            assert_cell_state!(prison, 3, 2, 0, MyNoCopy(3));
            assert_cell_state!(prison, 4, 2, 0, MyNoCopy(4));
            Ok(())
        })?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
        assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
        assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
        Ok(())
    })?;
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(4));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.visit_slice_ref(0..1, |_| Ok(())),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.visit_slice_ref(1..2, |_| Ok(())),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::guard_mut()
#[test]
fn prison_guard_mut() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(
        prison.guard_mut(CellKey::from_raw_parts(0, 0)),
        AccessError::IndexOutOfRange(0)
    );
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    {
        let mut val_0 = prison.guard_mut(key_0)?;
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
        *val_0 = MyNoCopy(10);
        assert_eq!(*val_0, MyNoCopy(10));
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_access_err!(
            prison.guard_mut(key_0),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(10));
    {
        let val_0 = prison.guard_ref(key_0)?;
        assert_access_err!(
            prison.guard_mut(key_0),
            AccessError::ValueStillImmutablyReferenced(0)
        );
    }
    let mut val_0 = prison.guard_mut(key_0)?;
    let mut val_1 = prison.guard_mut(key_1)?;
    let mut val_2 = prison.guard_mut(key_2)?;
    assert_eq!(*val_0, MyNoCopy(10));
    assert_eq!(*val_1, MyNoCopy(1));
    assert_eq!(*val_2, MyNoCopy(2));
    *val_0 = MyNoCopy(100);
    *val_1 = MyNoCopy(200);
    *val_2 = MyNoCopy(300);
    assert_eq!(*val_0, MyNoCopy(100));
    assert_eq!(*val_1, MyNoCopy(200));
    assert_eq!(*val_2, MyNoCopy(300));
    assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
    PrisonValueMut::unguard(val_0);
    PrisonValueMut::unguard(val_1);
    PrisonValueMut::unguard(val_2);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    prison.remove(key_0)?;
    assert_access_err!(prison.guard_mut(key_0), AccessError::ValueDeleted(0, 0));
    Ok(())
}

//TEST Prison::guard_ref()
#[test]
fn prison_guard_ref() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(
        prison.guard_ref(CellKey::from_raw_parts(0, 0)),
        AccessError::IndexOutOfRange(0)
    );
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    {
        let val_0 = prison.guard_ref(key_0)?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
    }
    {
        let val_0 = prison.guard_mut(key_0)?;
        assert_access_err!(
            prison.guard_ref(key_0),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    let val_0 = prison.guard_ref(key_0)?;
    let val_1 = prison.guard_ref(key_1)?;
    let val_2 = prison.guard_ref(key_2)?;
    assert_eq!(*val_0, MyNoCopy(0));
    assert_eq!(*val_1, MyNoCopy(1));
    assert_eq!(*val_2, MyNoCopy(2));
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    let val_2_b = prison.guard_ref(key_2)?;
    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
    let val_2_c = prison.guard_ref(key_2)?;
    assert_cell_state!(prison, 2, 3, 0, MyNoCopy(2));
    PrisonValueRef::unguard(val_2_b);
    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
    PrisonValueRef::unguard(val_2_c);
    PrisonValueRef::unguard(val_2);
    PrisonValueRef::unguard(val_0);
    PrisonValueRef::unguard(val_1);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    prison.remove(key_0)?;
    assert_access_err!(prison.guard_ref(key_0), AccessError::ValueDeleted(0, 0));
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.guard_ref(key_1),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::guard_mut_idx()
#[test]
fn prison_guard_mut_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(prison.guard_mut_idx(0), AccessError::IndexOutOfRange(0));
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    {
        let mut val_0 = prison.guard_mut_idx(0)?;
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
        *val_0 = MyNoCopy(10);
        assert_eq!(*val_0, MyNoCopy(10));
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_access_err!(
            prison.guard_mut_idx(0),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(10));
    {
        let val_0 = prison.guard_ref_idx(0)?;
        assert_access_err!(
            prison.guard_mut_idx(0),
            AccessError::ValueStillImmutablyReferenced(0)
        );
    }
    let mut val_0 = prison.guard_mut_idx(0)?;
    let mut val_1 = prison.guard_mut_idx(1)?;
    let mut val_2 = prison.guard_mut_idx(2)?;
    assert_eq!(*val_0, MyNoCopy(10));
    assert_eq!(*val_1, MyNoCopy(1));
    assert_eq!(*val_2, MyNoCopy(2));
    *val_0 = MyNoCopy(100);
    *val_1 = MyNoCopy(200);
    *val_2 = MyNoCopy(300);
    assert_eq!(*val_0, MyNoCopy(100));
    assert_eq!(*val_1, MyNoCopy(200));
    assert_eq!(*val_2, MyNoCopy(300));
    assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
    PrisonValueMut::unguard(val_0);
    PrisonValueMut::unguard(val_1);
    PrisonValueMut::unguard(val_2);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    prison.remove_idx(0)?;
    assert_access_err!(prison.guard_mut_idx(0), AccessError::ValueDeleted(0, 0));
    Ok(())
}

//TEST Prison::guard_ref_idx()
#[test]
fn prison_guard_ref_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
    assert_access_err!(
        prison.guard_ref(CellKey::from_raw_parts(0, 0)),
        AccessError::IndexOutOfRange(0)
    );
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    {
        let val_0 = prison.guard_ref_idx(0)?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*val_0, MyNoCopy(0));
    }
    {
        let val_0 = prison.guard_mut_idx(0)?;
        assert_access_err!(
            prison.guard_ref_idx(0),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    let val_0 = prison.guard_ref_idx(0)?;
    let val_1 = prison.guard_ref_idx(1)?;
    let val_2 = prison.guard_ref_idx(2)?;
    assert_eq!(*val_0, MyNoCopy(0));
    assert_eq!(*val_1, MyNoCopy(1));
    assert_eq!(*val_2, MyNoCopy(2));
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    let val_2_b = prison.guard_ref_idx(2)?;
    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
    let val_2_c = prison.guard_ref_idx(2)?;
    assert_cell_state!(prison, 2, 3, 0, MyNoCopy(2));
    PrisonValueRef::unguard(val_2_b);
    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
    PrisonValueRef::unguard(val_2_c);
    PrisonValueRef::unguard(val_2);
    PrisonValueRef::unguard(val_0);
    PrisonValueRef::unguard(val_1);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    prison.remove_idx(0)?;
    assert_access_err!(prison.guard_ref_idx(0), AccessError::ValueDeleted(0, 0));
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.guard_ref_idx(1),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::guard_many_mut()
#[test]
fn prison_guard_many_mut() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.guard_many_mut(&[CellKey::from_raw_parts(0, 0)]),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.guard_many_mut(&[]).is_ok());
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    let key_3 = prison.insert(MyNoCopy(3))?;
    let key_4 = prison.insert(MyNoCopy(4))?;
    {
        let mut vals_0_1 = prison.guard_many_mut(&[key_0, key_1])?;
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        *vals_0_1[0] = MyNoCopy(10);
        *vals_0_1[1] = MyNoCopy(11);
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_eq!(*vals_0_1[0], MyNoCopy(10));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(11));
        assert_eq!(*vals_0_1[1], MyNoCopy(11));
        assert_access_err!(
            prison.guard_many_mut(&[key_0]),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    {
        let val_0 = prison.guard_ref(key_0)?;
        assert_access_err!(
            prison.guard_many_mut(&[key_0, key_1]),
            AccessError::ValueStillImmutablyReferenced(0)
        );
    }
    let mut vals_0_2_4 = prison.guard_many_mut(&[key_0, key_2, key_4])?;
    let mut vals_1_3 = prison.guard_many_mut(&[key_1, key_3])?;
    assert_eq!(*vals_0_2_4[0], MyNoCopy(10));
    assert_eq!(*vals_1_3[0], MyNoCopy(11));
    assert_eq!(*vals_0_2_4[1], MyNoCopy(2));
    assert_eq!(*vals_1_3[1], MyNoCopy(3));
    assert_eq!(*vals_0_2_4[2], MyNoCopy(4));
    *vals_0_2_4[0] = MyNoCopy(100);
    *vals_1_3[0] = MyNoCopy(200);
    *vals_0_2_4[1] = MyNoCopy(300);
    *vals_1_3[1] = MyNoCopy(400);
    *vals_0_2_4[2] = MyNoCopy(500);
    assert_eq!(*vals_0_2_4[0], MyNoCopy(100));
    assert_eq!(*vals_1_3[0], MyNoCopy(200));
    assert_eq!(*vals_0_2_4[1], MyNoCopy(300));
    assert_eq!(*vals_1_3[1], MyNoCopy(400));
    assert_eq!(*vals_0_2_4[2], MyNoCopy(500));
    assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, Refs::MUT, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, Refs::MUT, 0, MyNoCopy(500));
    PrisonSliceMut::unguard(vals_1_3);
    PrisonSliceMut::unguard(vals_0_2_4);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(500));
    prison.remove(key_0)?;
    assert_access_err!(
        prison.guard_many_mut(&[key_0]),
        AccessError::ValueDeleted(0, 0)
    );
    Ok(())
}

//TEST Prison::guard_many_ref()
#[test]
fn prison_guard_many_ref() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.guard_many_ref(&[CellKey::from_raw_parts(0, 0)]),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.guard_many_ref(&[]).is_ok());
    let key_0 = prison.insert(MyNoCopy(0))?;
    let key_1 = prison.insert(MyNoCopy(1))?;
    let key_2 = prison.insert(MyNoCopy(2))?;
    let key_3 = prison.insert(MyNoCopy(3))?;
    let key_4 = prison.insert(MyNoCopy(4))?;
    {
        let vals_0_1 = prison.guard_many_ref(&[key_0, key_1])?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
    }
    {
        let val_0 = prison.guard_mut(key_0)?;
        assert_access_err!(
            prison.guard_many_ref(&[key_0, key_1]),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    let vals_a = prison.guard_many_ref(&[key_0, key_1, key_2, key_3, key_4])?;
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
    let vals_b = prison.guard_many_ref(&[key_0, key_1, key_2, key_3, key_4])?;
    assert_eq!(*vals_a[0], MyNoCopy(0));
    assert_eq!(*vals_b[1], MyNoCopy(1));
    assert_eq!(*vals_a[2], MyNoCopy(2));
    assert_eq!(*vals_b[3], MyNoCopy(3));
    assert_eq!(*vals_a[4], MyNoCopy(4));
    assert_cell_state!(prison, 0, 2, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 2, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 2, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 2, 0, MyNoCopy(4));
    PrisonSliceRef::unguard(vals_b);
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
    PrisonSliceRef::unguard(vals_a);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(4));
    prison.remove(key_0)?;
    assert_access_err!(
        prison.guard_many_ref(&[key_0]),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.guard_many_ref(&[key_1]),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::guard_many_mut_idx()
#[test]
fn prison_guard_many_mut_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.guard_many_mut_idx(&[0]),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.guard_many_mut_idx(&[]).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    {
        let mut vals_0_1 = prison.guard_many_mut_idx(&[0, 1])?;
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        *vals_0_1[0] = MyNoCopy(10);
        *vals_0_1[1] = MyNoCopy(11);
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_eq!(*vals_0_1[0], MyNoCopy(10));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(11));
        assert_eq!(*vals_0_1[1], MyNoCopy(11));
        assert_access_err!(
            prison.guard_many_mut_idx(&[0]),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    {
        let val_0 = prison.guard_ref_idx(0)?;
        assert_access_err!(
            prison.guard_many_mut_idx(&[0, 1]),
            AccessError::ValueStillImmutablyReferenced(0)
        );
    }
    let mut vals_0_2_4 = prison.guard_many_mut_idx(&[0, 2, 4])?;
    let mut vals_1_3 = prison.guard_many_mut_idx(&[1, 3])?;
    assert_eq!(*vals_0_2_4[0], MyNoCopy(10));
    assert_eq!(*vals_1_3[0], MyNoCopy(11));
    assert_eq!(*vals_0_2_4[1], MyNoCopy(2));
    assert_eq!(*vals_1_3[1], MyNoCopy(3));
    assert_eq!(*vals_0_2_4[2], MyNoCopy(4));
    *vals_0_2_4[0] = MyNoCopy(100);
    *vals_1_3[0] = MyNoCopy(200);
    *vals_0_2_4[1] = MyNoCopy(300);
    *vals_1_3[1] = MyNoCopy(400);
    *vals_0_2_4[2] = MyNoCopy(500);
    assert_eq!(*vals_0_2_4[0], MyNoCopy(100));
    assert_eq!(*vals_1_3[0], MyNoCopy(200));
    assert_eq!(*vals_0_2_4[1], MyNoCopy(300));
    assert_eq!(*vals_1_3[1], MyNoCopy(400));
    assert_eq!(*vals_0_2_4[2], MyNoCopy(500));
    assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, Refs::MUT, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, Refs::MUT, 0, MyNoCopy(500));
    PrisonSliceMut::unguard(vals_1_3);
    PrisonSliceMut::unguard(vals_0_2_4);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(500));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.guard_many_mut_idx(&[0]),
        AccessError::ValueDeleted(0, 0)
    );
    Ok(())
}

//TEST Prison::guard_many_ref_idx()
#[test]
fn prison_guard_many_ref_idx() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.guard_many_ref_idx(&[0]),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.guard_many_ref_idx(&[]).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    {
        let vals_0_1 = prison.guard_many_ref_idx(&[0, 1])?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
    }
    {
        let val_0 = prison.guard_mut_idx(0)?;
        assert_access_err!(
            prison.guard_many_ref_idx(&[0, 1]),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    let vals_a = prison.guard_many_ref_idx(&[0, 1, 2, 3, 4])?;
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
    let vals_b = prison.guard_many_ref_idx(&[0, 1, 2, 3, 4])?;
    assert_eq!(*vals_a[0], MyNoCopy(0));
    assert_eq!(*vals_b[1], MyNoCopy(1));
    assert_eq!(*vals_a[2], MyNoCopy(2));
    assert_eq!(*vals_b[3], MyNoCopy(3));
    assert_eq!(*vals_a[4], MyNoCopy(4));
    assert_cell_state!(prison, 0, 2, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 2, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 2, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 2, 0, MyNoCopy(4));
    PrisonSliceRef::unguard(vals_b);
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
    PrisonSliceRef::unguard(vals_a);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(4));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.guard_many_ref_idx(&[0]),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.guard_many_ref_idx(&[1]),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::guard_slice_mut()
#[test]
fn prison_guard_slice_mut() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.guard_slice_mut(0..1),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.guard_slice_mut(..).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    {
        let mut vals_0_1 = prison.guard_slice_mut(0..=1)?;
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
        *vals_0_1[0] = MyNoCopy(10);
        *vals_0_1[1] = MyNoCopy(11);
        assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(10));
        assert_eq!(*vals_0_1[0], MyNoCopy(10));
        assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(11));
        assert_eq!(*vals_0_1[1], MyNoCopy(11));
        assert_access_err!(
            prison.guard_slice_mut(0..1),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    {
        let val_0 = prison.guard_ref_idx(0)?;
        assert_access_err!(
            prison.guard_slice_mut(0..1),
            AccessError::ValueStillImmutablyReferenced(0)
        );
    }
    let mut vals_0_1_2 = prison.guard_slice_mut(..3)?;
    let mut vals_3_4 = prison.guard_slice_mut(3..)?;
    assert_eq!(*vals_0_1_2[0], MyNoCopy(10));
    assert_eq!(*vals_0_1_2[1], MyNoCopy(11));
    assert_eq!(*vals_0_1_2[2], MyNoCopy(2));
    assert_eq!(*vals_3_4[0], MyNoCopy(3));
    assert_eq!(*vals_3_4[1], MyNoCopy(4));
    *vals_0_1_2[0] = MyNoCopy(100);
    *vals_0_1_2[1] = MyNoCopy(200);
    *vals_0_1_2[2] = MyNoCopy(300);
    *vals_3_4[0] = MyNoCopy(400);
    *vals_3_4[1] = MyNoCopy(500);
    assert_eq!(*vals_0_1_2[0], MyNoCopy(100));
    assert_eq!(*vals_0_1_2[1], MyNoCopy(200));
    assert_eq!(*vals_0_1_2[2], MyNoCopy(300));
    assert_eq!(*vals_3_4[0], MyNoCopy(400));
    assert_eq!(*vals_3_4[1], MyNoCopy(500));
    assert_cell_state!(prison, 0, Refs::MUT, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, Refs::MUT, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, Refs::MUT, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, Refs::MUT, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, Refs::MUT, 0, MyNoCopy(500));
    PrisonSliceMut::unguard(vals_0_1_2);
    PrisonSliceMut::unguard(vals_3_4);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(100));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(200));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(300));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(400));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(500));
    prison.remove_idx(0)?;
    assert_access_err!(prison.guard_slice_mut(..), AccessError::ValueDeleted(0, 0));
    Ok(())
}

//TEST Prison::guard_slice_ref()
#[test]
fn prison_guard_slice_ref() -> Result<(), AccessError> {
    let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
    assert_access_err!(
        prison.guard_slice_ref(0..1),
        AccessError::IndexOutOfRange(0)
    );
    assert!(prison.guard_slice_ref(..).is_ok());
    prison.insert(MyNoCopy(0))?;
    prison.insert(MyNoCopy(1))?;
    prison.insert(MyNoCopy(2))?;
    prison.insert(MyNoCopy(3))?;
    prison.insert(MyNoCopy(4))?;
    {
        let vals_0_1 = prison.guard_slice_ref(0..=1)?;
        assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
        assert_eq!(*vals_0_1[0], MyNoCopy(0));
        assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
        assert_eq!(*vals_0_1[1], MyNoCopy(1));
    }
    {
        let val_0 = prison.guard_mut_idx(0)?;
        assert_access_err!(
            prison.guard_slice_ref(0..1),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    let vals_a = prison.guard_slice_ref(..)?;
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
    let vals_b = prison.guard_slice_ref(..)?;
    assert_eq!(*vals_a[0], MyNoCopy(0));
    assert_eq!(*vals_b[1], MyNoCopy(1));
    assert_eq!(*vals_a[2], MyNoCopy(2));
    assert_eq!(*vals_b[3], MyNoCopy(3));
    assert_eq!(*vals_a[4], MyNoCopy(4));
    assert_cell_state!(prison, 0, 2, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 2, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 2, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 2, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 2, 0, MyNoCopy(4));
    PrisonSliceRef::unguard(vals_b);
    assert_cell_state!(prison, 0, 1, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 1, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 1, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 1, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 1, 0, MyNoCopy(4));
    PrisonSliceRef::unguard(vals_a);
    assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
    assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
    assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
    assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
    assert_cell_state!(prison, 4, 0, 0, MyNoCopy(4));
    prison.remove_idx(0)?;
    assert_access_err!(
        prison.guard_slice_ref(0..1),
        AccessError::ValueDeleted(0, 0)
    );
    internal!(prison).vec[1].refs_or_next = Refs::MAX_IMMUT;
    assert_access_err!(
        prison.guard_slice_ref(1..2),
        AccessError::MaximumImmutableReferencesReached(1)
    );
    Ok(())
}

//TEST Prison::clone_val()
#[test]
fn prison_clone_val() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::with_capacity(5);
    let key_0 = prison.insert(String::from("The"))?;
    let key_1 = prison.insert(String::from("quick"))?;
    let key_2 = prison.insert(String::from("red"))?;
    let key_3 = prison.insert(String::from("fox"))?;
    let key_4 = prison.insert(String::from("jumped"))?;
    let mut jumped: String = String::new();
    let mut jumped_2: String = String::new();
    prison.visit_mut(key_4, |val_4| {
        jumped = prison.clone_val(key_4)?;
        jumped_2 = val_4.clone();
        *val_4 = String::from("skipped");
        Ok(())
    })?;
    assert_eq!(jumped, String::from("jumped"));
    assert_eq!(jumped, jumped_2);
    jumped = String::from("fell");
    jumped_2 = String::from("exploded");
    assert_cell_state!(prison, 4, 0, 0, String::from("skipped"));
    assert_access_err!(
        prison.clone_val(CellKey::from_raw_parts(5, 0)),
        AccessError::IndexOutOfRange(5)
    );
    prison.remove(key_3)?;
    assert_access_err!(prison.clone_val(key_3), AccessError::ValueDeleted(3, 0));
    let key_3_b = prison.insert(String::from("orange"))?;
    assert_access_err!(prison.clone_val(key_3), AccessError::ValueDeleted(3, 0));
    Ok(())
}

//TEST Prison::clone_val_idx()
#[test]
fn prison_clone_val_idx() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::with_capacity(5);
    prison.insert(String::from("The"))?;
    prison.insert(String::from("quick"))?;
    prison.insert(String::from("red"))?;
    prison.insert(String::from("fox"))?;
    prison.insert(String::from("jumped"))?;
    let mut jumped: String = String::new();
    let mut jumped_2: String = String::new();
    prison.visit_mut_idx(4, |val_4| {
        jumped = prison.clone_val_idx(4)?;
        jumped_2 = val_4.clone();
        *val_4 = String::from("skipped");
        Ok(())
    })?;
    assert_eq!(jumped, String::from("jumped"));
    assert_eq!(jumped, jumped_2);
    jumped = String::from("fell");
    jumped_2 = String::from("exploded");
    assert_cell_state!(prison, 4, 0, 0, String::from("skipped"));
    assert_access_err!(
        prison.clone_val(CellKey::from_raw_parts(5, 0)),
        AccessError::IndexOutOfRange(5)
    );
    prison.remove_idx(3)?;
    assert_access_err!(prison.clone_val_idx(3), AccessError::ValueDeleted(3, 0));
    Ok(())
}

//TEST Prison::clone_many_vals()
#[test]
fn prison_clone_many_vals() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::with_capacity(5);
    let key_0 = prison.insert(String::from("The"))?;
    let key_1 = prison.insert(String::from("quick"))?;
    let key_2 = prison.insert(String::from("red"))?;
    let key_3 = prison.insert(String::from("fox"))?;
    let key_4 = prison.insert(String::from("jumped"))?;
    let mut words_1: Vec<String> = Vec::new();
    let mut words_2: Vec<String> = Vec::new();
    prison.visit_slice_mut(.., |vals| {
        words_1 = prison.clone_many_vals(&[key_0, key_1, key_2, key_3, key_4])?;
        for word in vals {
            words_2.push(word.clone());
            **word = String::from("null")
        }
        Ok(())
    })?;
    let sentence_1 = words_1.iter().fold(String::new(), |mut sentence, word| {
        sentence.push_str(&word);
        sentence.push(' ');
        sentence
    });
    let sentence_2 = words_2.iter().fold(String::new(), |mut sentence, word| {
        sentence.push_str(&word);
        sentence.push(' ');
        sentence
    });
    assert_eq!(sentence_1, String::from("The quick red fox jumped "));
    assert_eq!(sentence_1, sentence_2);
    words_1[4] = String::from("fell");
    words_2[4] = String::from("exploded");
    assert_cell_state!(prison, 4, 0, 0, String::from("null"));
    assert_access_err!(
        prison.clone_many_vals(&[CellKey::from_raw_parts(5, 0)]),
        AccessError::IndexOutOfRange(5)
    );
    prison.remove(key_3)?;
    assert_access_err!(
        prison.clone_many_vals(&[key_0, key_1, key_2, key_3, key_4]),
        AccessError::ValueDeleted(3, 0)
    );
    let key_3_b = prison.insert(String::from("nil"))?;
    assert_access_err!(
        prison.clone_many_vals(&[key_0, key_1, key_2, key_3, key_4]),
        AccessError::ValueDeleted(3, 0)
    );
    Ok(())
}

//TEST Prison::clone_many_vals_idx()
#[test]
fn prison_clone_many_vals_idx() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::with_capacity(5);
    prison.insert(String::from("The"))?;
    prison.insert(String::from("quick"))?;
    prison.insert(String::from("red"))?;
    prison.insert(String::from("fox"))?;
    prison.insert(String::from("jumped"))?;
    let mut words_1: Vec<String> = Vec::new();
    let mut words_2: Vec<String> = Vec::new();
    prison.visit_slice_mut(.., |vals| {
        words_1 = prison.clone_many_vals_idx(&[0, 1, 2, 3, 4])?;
        for word in vals {
            words_2.push(word.clone());
            **word = String::from("null")
        }
        Ok(())
    })?;
    let sentence_1 = words_1.iter().fold(String::new(), |mut sentence, word| {
        sentence.push_str(&word);
        sentence.push(' ');
        sentence
    });
    let sentence_2 = words_2.iter().fold(String::new(), |mut sentence, word| {
        sentence.push_str(&word);
        sentence.push(' ');
        sentence
    });
    assert_eq!(sentence_1, String::from("The quick red fox jumped "));
    assert_eq!(sentence_1, sentence_2);
    words_1[4] = String::from("fell");
    words_2[4] = String::from("exploded");
    assert_cell_state!(prison, 4, 0, 0, String::from("null"));
    assert_access_err!(
        prison.clone_many_vals_idx(&[5]),
        AccessError::IndexOutOfRange(5)
    );
    prison.remove_idx(3)?;
    assert_access_err!(
        prison.clone_many_vals_idx(&[0, 1, 2, 3, 4]),
        AccessError::ValueDeleted(3, 0)
    );
    Ok(())
}

//TEST Prison::peek_ref()
#[test]
fn prison_peek_ref() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::with_capacity(5);
    let key_0 = prison.insert(String::from("The"))?;
    let key_1 = prison.insert(String::from("quick"))?;
    let key_2 = prison.insert(String::from("red"))?;
    let key_3 = prison.insert(String::from("fox"))?;
    let key_4 = prison.insert(String::from("jumped"))?;
    prison.visit_mut(key_4, |val_4| {
        let jumped_ref = unsafe {prison.peek_ref(key_4)?};
        assert_eq!(*jumped_ref, String::from("jumped"));
        assert_cell_state!(prison, 4, Refs::MUT, 0, String::from("jumped"));
        Ok(())
    })?;
    assert_cell_state!(prison, 4, 0, 0, String::from("jumped"));
    Ok(())
}

//TEST Prison::peek_ref_idx()
#[test]
fn prison_peek_ref_idx() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::with_capacity(5);
    prison.insert(String::from("The"))?;
    prison.insert(String::from("quick"))?;
    prison.insert(String::from("red"))?;
    prison.insert(String::from("fox"))?;
    prison.insert(String::from("jumped"))?;
    prison.visit_mut_idx(4, |val_4| {
        let jumped_ref = unsafe {prison.peek_ref_idx(4)?};
        assert_eq!(*jumped_ref, String::from("jumped"));
        assert_cell_state!(prison, 4, Refs::MUT, 0, String::from("jumped"));
        Ok(())
    })?;
    assert_cell_state!(prison, 4, 0, 0, String::from("jumped"));
    Ok(())
}

//------ JailCell Tests ------
//TODO: TEST JailCell::new()

//TEST JailCell::visit_mut()
#[test]
fn jail_visit_mut() -> Result<(), AccessError> {
    let jail: JailCell<MyNoCopy> = JailCell::new(MyNoCopy(42));
    jail.visit_mut(|val| {
        assert_jail_state!(jail, Refs::MUT, MyNoCopy(42));
        assert_eq!(*val, MyNoCopy(42));
        *val = MyNoCopy(69);
        assert_eq!(*val, MyNoCopy(69));
        assert_jail_state!(jail, Refs::MUT, MyNoCopy(69));
        assert_access_err!(
            jail.visit_mut(|_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    assert_jail_state!(jail, 0, MyNoCopy(69));
    jail.visit_ref(|val| {
        assert_access_err!(
            jail.visit_mut(|_| Ok(())),
            AccessError::ValueStillImmutablyReferenced(0)
        );
        Ok(())
    })?;
    Ok(())
}

//TEST JailCell::visit_ref()
#[test]
fn jail_visit_ref() -> Result<(), AccessError> {
    let jail: JailCell<MyNoCopy> = JailCell::new(MyNoCopy(42));
    jail.visit_ref(|val| {
        assert_jail_state!(jail, 1, MyNoCopy(42));
        assert_eq!(*val, MyNoCopy(42));
        jail.visit_ref(|val_b| {
            assert_jail_state!(jail, 2, MyNoCopy(42));
            assert_eq!(*val_b, MyNoCopy(42));
            Ok(())
        })?;
        assert_jail_state!(jail, 1, MyNoCopy(42));
        Ok(())
    })?;
    assert_jail_state!(jail, 0, MyNoCopy(42));
    jail.visit_mut(|val| {
        assert_access_err!(
            jail.visit_ref(|_| Ok(())),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    internal!(jail).refs = Refs::MAX_IMMUT;
    assert_access_err!(
        jail.visit_ref(|_| Ok(())),
        AccessError::MaximumImmutableReferencesReached(0)
    );
    Ok(())
}

//TEST JailCell::guard_mut()
#[test]
fn jail_guard_mut() -> Result<(), AccessError> {
    let jail: JailCell<MyNoCopy> = JailCell::new(MyNoCopy(42));
    {
        let mut val = jail.guard_mut()?;
        assert_jail_state!(jail, Refs::MUT, MyNoCopy(42));
        assert_eq!(*val, MyNoCopy(42));
        *val = MyNoCopy(69);
        assert_eq!(*val, MyNoCopy(69));
        assert_jail_state!(jail, Refs::MUT, MyNoCopy(69));
        assert_access_err!(
            jail.guard_mut(),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
    }
    assert_jail_state!(jail, 0, MyNoCopy(69));
    let mut val = jail.guard_mut()?;
    assert_jail_state!(jail, Refs::MUT, MyNoCopy(69));
    *val = MyNoCopy(42);
    JailValueMut::unguard(val);
    assert_jail_state!(jail, 0, MyNoCopy(42));
    jail.visit_ref(|val| {
        assert_access_err!(
            jail.guard_mut(),
            AccessError::ValueStillImmutablyReferenced(0)
        );
        Ok(())
    })?;
    Ok(())
}

//TEST JailCell::guard_ref()
#[test]
fn jail_guard_ref() -> Result<(), AccessError> {
    let jail: JailCell<MyNoCopy> = JailCell::new(MyNoCopy(42));
    {
        let val = jail.guard_ref()?;
        assert_jail_state!(jail, 1, MyNoCopy(42));
        assert_eq!(*val, MyNoCopy(42));
        let val_b = jail.guard_ref()?;
        assert_jail_state!(jail, 2, MyNoCopy(42));
        assert_eq!(*val_b, MyNoCopy(42));
        JailValueRef::unguard(val_b);
        assert_jail_state!(jail, 1, MyNoCopy(42));
    }
    assert_jail_state!(jail, 0, MyNoCopy(42));
    jail.visit_mut(|val| {
        assert_access_err!(
            jail.guard_ref(),
            AccessError::ValueAlreadyMutablyReferenced(0)
        );
        Ok(())
    })?;
    internal!(jail).refs = Refs::MAX_IMMUT;
    assert_access_err!(
        jail.guard_ref(),
        AccessError::MaximumImmutableReferencesReached(0)
    );
    Ok(())
}

//TEST JailCell::clone_val()
#[test]
fn jail_clone_val() -> Result<(), AccessError> {
    let jail: JailCell<String> = JailCell::new(String::from("fox"));
    let mut animal_1: String = String::new();
    let mut animal_2: String = String::new();
    jail.visit_mut(|val| {
        animal_1 = jail.clone_val();
        animal_2 = val.clone();
        *val = String::from("dog");
        Ok(())
    })?;
    assert_eq!(animal_1, String::from("fox"));
    assert_eq!(animal_1, animal_2);
    animal_1 = String::from("bear");
    animal_2 = String::from("cat");
    assert_jail_state!(jail, 0, String::from("dog"));
    Ok(())
}

//TEST JailCell::peek_ref()
#[test]
fn jail_peek_ref() -> Result<(), AccessError> {
    let jail: JailCell<String> = JailCell::new(String::from("fox"));
    jail.visit_mut(|val| {
        let unsafe_ref = unsafe {jail.peek_ref()};
        assert_eq!(*unsafe_ref, String::from("fox"));
        assert_jail_state!(jail, Refs::MUT, String::from("fox"));
        Ok(())
    })?;
    assert_jail_state!(jail, 0, String::from("fox"));
    Ok(())
}