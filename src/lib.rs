/******************************************************************************
 * Copyright (c) 2014-2016, Pedro Ramalhete, Andreia Correia
 * All rights reserved.
 *
 * Rust Version by Junker Jörg
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *     * Redistributions of source code must retain the above copyright
 *       notice, this list of conditions and the following disclaimer.
 *     * Redistributions in binary form must reproduce the above copyright
 *       notice, this list of conditions and the following disclaimer in the
 *       documentation and/or other materials provided with the distribution.
 *     * Neither the name of Concurrency Freaks nor the
 *       names of its contributors may be used to endorse or promote products
 *       derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL <COPYRIGHT HOLDER> BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 ******************************************************************************
 */

use array_macro::array;
use peril::{HazardPointer, HazardRecord, HazardRegistry, HazardValue, Ordering};
use std::sync::atomic::{AtomicPtr, AtomicU32};

#[cfg(test)]
mod tests;
/**
 * <h1> Fetch-And-Add Array Queue </h1>
 *
 * Each node has one array but we don't search for a vacant entry. Instead, we
 * use FAA to obtain an index in the array, for enqueueing or dequeuing.
 *
 * There are some similarities between this queue and the basic queue in YMC:
 * http://chaoran.me/assets/pdf/wfq-ppopp16.pdf
 * but it's not the same because the queue in listing 1 is obstruction-free, while
 * our algorithm is lock-free.
 * In FAAArrayQueue eventually a new node will be inserted (using Michael-Scott's
 * algorithm) and it will have an item pre-filled in the first position, which means
 * that at most, after BUFFER_SIZE steps, one item will be enqueued (and it can then
 * be dequeued). This kind of progress is lock-free.
 *
 * Each entry in the array may contain one of three possible values:
 * - A valid item that has been enqueued;
 * - nullptr, which means no item has yet been enqueued in that position;
 * - taken, a special value that means there was an item but it has been dequeued;
 *
 * Enqueue algorithm: FAA + CAS(null,item)
 * Dequeue algorithm: FAA + CAS(item,taken)
 * Consistency: Linearizable
 * enqueue() progress: lock-free
 * dequeue() progress: lock-free
 * Memory Reclamation: Hazard Pointers (lock-free)
 * Uncontended enqueue: 1 FAA + 1 CAS + 1 HP
 * Uncontended dequeue: 1 FAA + 1 CAS + 1 HP
 *
 *
 * <p>
 * Lock-Free Linked List as described in Maged Michael and Michael Scott's paper:
 * {@link http://www.cs.rochester.edu/~scott/papers/1996_PODC_queues.pdf}
 * <a href="http://www.cs.rochester.edu/~scott/papers/1996_PODC_queues.pdf">
 * Simple, Fast, and Practical Non-Blocking and Blocking Concurrent Queue Algorithms</a>
 * <p>
 * The paper on Hazard Pointers is named "Hazard Pointers: Safe Memory
 * Reclamation for Lock-Free objects" and it is available here:
 * http://web.cecs.pdx.edu/~walpole/class/cs510/papers/11.pdf
 *
 * @author Pedro Ramalhete
 * @author Andreia Correia
 */

const BUFFER_SIZE: u32 = 1024;
const TAKEN: *mut () = 1 as *mut ();

struct Node<T> {
    deqidx: AtomicU32,
    items: [AtomicPtr<T>; BUFFER_SIZE as usize],
    enqidx: AtomicU32,
    next: HazardPointer<Node<T>>,
}

impl<T: Send> Node<T> {
    fn new(item: *mut T) -> Self {
        let items: [AtomicPtr<T>; BUFFER_SIZE as usize] =
            array![_ => Default::default(); BUFFER_SIZE as usize];
        items[0].store(item, Ordering::Relaxed);
        Node {
            deqidx: AtomicU32::new(0),
            items,
            enqidx: AtomicU32::new(1),
            next: HazardPointer::new(HazardValue::dummy(0)),
        }
    }

    fn cas_next<'registry>(
        &self,
        registry: &'registry HazardRegistry<Node<T>>,
        cmp: HazardValue<Node<T>>,
        val: HazardValue<Node<T>>,
    ) -> bool {
        self.next
            .compare_exchange(registry, cmp, val, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }
}
#[repr(align(128))]
struct AlignedHazardPtr<T: Send>(HazardPointer<T>);

/// a lock free mpmc queue
///
/// # Examples
///
/// ```
/// use faa_array_queue::FaaArrayQueue;
///
/// let queue = FaaArrayQueue::<usize>::default();
/// ```
pub struct FaaArrayQueue<T: Send> {
    registry: HazardRegistry<Node<T>>,
    head: AlignedHazardPtr<Node<T>>,
    tail: AlignedHazardPtr<Node<T>>,
}

impl<T: Send> Drop for FaaArrayQueue<T> {
    fn drop(&mut self) {
        loop {
            if self.dequeue().is_none() {
                break;
            }
        }
    }
}

impl<T: Send> Default for FaaArrayQueue<T> {
    fn default() -> Self {
        let registry = HazardRegistry::default();
        let sentinel = HazardValue::boxed(Node::new(std::ptr::null_mut()));
        let head = AlignedHazardPtr(HazardPointer::new(sentinel.clone()));
        let tail = AlignedHazardPtr(HazardPointer::new(sentinel));
        FaaArrayQueue {
            registry,
            head,
            tail,
        }
    }
}

impl<T: Send> FaaArrayQueue<T> {
    /// add an item to the tail of the queue
    ///
    /// # Arguments
    ///
    /// * `item` - the item to add
    ///
    /// # Examples
    ///
    /// ```
    /// use faa_array_queue::FaaArrayQueue;
    ///
    /// let queue = FaaArrayQueue::<usize>::default();
    /// queue.enqueue(1337);
    /// assert!(queue.dequeue().unwrap() == 1337);
    /// ```
    pub fn enqueue(&self, item: T) {
        let item = Box::new(item);
        let item = Box::into_raw(item);
        let mut record = HazardRecord::default();
        loop {
            let scope = self.tail.0.protect(&self.registry, &mut record);
            let ltail = scope.as_ref().unwrap();
            let idx = ltail.enqidx.fetch_add(1, Ordering::AcqRel);
            if idx > (BUFFER_SIZE - 1) {
                // This node is full
                if scope.changed(Ordering::Acquire) {
                    drop(scope);
                    continue;
                }

                let lnext = {
                    let mut record2 = HazardRecord::default();
                    let scope2 = ltail.next.protect(&self.registry, &mut record2);
                    scope2.clone_value()
                };

                if lnext.is_dummy() {
                    let new_node = HazardValue::boxed(Node::new(item));
                    let cloned_node = new_node.clone();
                    if ltail.cas_next(&self.registry, HazardValue::dummy(0), new_node) {
                        let _ = scope.compare_exchange(
                            cloned_node,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        );
                        return;
                    }
                } else {
                    let _ = scope.compare_exchange(lnext, Ordering::AcqRel, Ordering::Relaxed);
                }
                continue;
            }

            if ltail.items[idx as usize]
                .compare_exchange(
                    std::ptr::null_mut(),
                    item,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return;
            }
        }
    }

    /// remove an item from the head of the queue
    ///
    /// # Arguments
    ///
    /// * `return` - Some item removed or None if the queue is empty
    ///
    /// # Examples
    ///
    /// ```
    /// use faa_array_queue::FaaArrayQueue;
    ///
    /// let queue = FaaArrayQueue::<usize>::default();
    /// assert!(queue.dequeue().is_none());
    /// ```
    pub fn dequeue(&self) -> Option<T> {
        let mut record = HazardRecord::default();
        loop {
            let scope = self.head.0.protect(&self.registry, &mut record);
            let lhead = scope.as_ref().unwrap();

            if lhead.deqidx.load(Ordering::Acquire) >= lhead.enqidx.load(Ordering::Acquire)
                && lhead.next.get_dummy(Ordering::Acquire).is_none()
            {
                break;
            }
            let idx = lhead.deqidx.fetch_add(1, Ordering::AcqRel);
            if idx > (BUFFER_SIZE - 1) {
                // This node has been drained, check if there is another one
                let lnext = {
                    let mut record2 = HazardRecord::default();
                    let scope2 = lhead.next.protect(&self.registry, &mut record2);
                    scope2.clone_value()
                };
                if lnext.is_dummy() {
                    break; // No more nodes in the queue
                }

                let _ = scope.compare_exchange(lnext, Ordering::AcqRel, Ordering::Relaxed);
                continue;
            }

            let item = lhead.items[idx as usize].swap(TAKEN as *mut T, Ordering::AcqRel);
            if item == std::ptr::null_mut() {
                continue;
            }
            let item = unsafe { Box::from_raw(item) };
            return Some(*item);
        }
        None
    }
}
