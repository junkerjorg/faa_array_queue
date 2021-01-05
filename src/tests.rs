use crate::FaaArrayQueue;

#[test]
pub fn test_hazard_vector() {
    use std::sync::Arc;
    use std::thread;

    let thread_count = 128;
    let loop_count = 53;
    let queue1 = Arc::new(FaaArrayQueue::<usize>::default());
    let queue2 = Arc::new(FaaArrayQueue::<usize>::default());

    let handles: Vec<_> = (0..thread_count)
        .into_iter()
        .map(|idx| {
            let cpy1 = queue1.clone();
            let cpy2 = queue2.clone();
            thread::spawn(move || {
                thread::park();
                let start = idx * loop_count;
                for idx in start..start + loop_count {
                    cpy1.enqueue(idx);
                }
                for _ in 0..loop_count {
                    let val;
                    loop {
                        if let Some(x) = cpy1.dequeue() {
                            val = x;
                            break;
                        }
                    }
                    cpy2.enqueue(val);
                }
            })
        })
        .collect();

    for child in handles.iter() {
        child.thread().unpark();
    }

    for child in handles.into_iter() {
        child.join().unwrap();
    }

    let none = queue1.dequeue();
    assert!(none.is_none(), "expected vec1 to be empty");
    let mut vec2 = Vec::new();
    loop {
        if let Some(x) = queue2.dequeue() {
            vec2.push(x);
        } else {
            break;
        }
    }

    vec2.sort();

    for i in 0..(thread_count * loop_count) {
        assert!(vec2[i] == i, "unexpected vec2 value");
    }
}
