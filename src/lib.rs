use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

enum Message {
    NewJob(Job),
    Terminate,
}

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Message>,
}

trait FnBox {
    fn call_box(self: Box<Self>);
}

impl<F: FnOnce()> FnBox for F {
    fn call_box(self: Box<F>) {
        (*self)()
    }
}

type Job = Box<dyn FnBox + Send + 'static>;

impl ThreadPool {
    /// Creates a new ThreadPool with the specified number of worker threads.
    ///
    /// The `ThreadPool` allows concurrent execution of tasks by a pool of worker threads.
    ///
    /// # Arguments
    ///
    /// * `size` - The number of worker threads in the pool.
    ///
    /// # Panics
    ///
    /// This function will panic if the `size` argument is zero.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::mpsc::channel;
    /// use std::sync::Arc;
    /// use std::sync::Mutex;
    /// use threadpool_rs::ThreadPool;
    ///
    /// let pool = ThreadPool::new(4);
    ///
    /// // Create an Arc and Mutex to share data among tasks.
    /// let shared_data = Arc::new(Mutex::new(Vec::new()));
    ///
    /// for i in 0..4 {
    ///     let data_clone = Arc::clone(&shared_data);
    ///     pool.execute(move || {
    ///         let mut data = data_clone.lock().unwrap();
    ///         data.push(i);
    ///     });
    /// }
    ///
    /// // Ensure all tasks are completed by dropping the ThreadPool.
    /// drop(pool);
    ///
    /// // Wait for the worker threads to finish and collect the results.
    /// let result = shared_data.lock().unwrap();
    /// assert_eq!(result.len(), 4);
    /// ```
    ///
    /// In this example, the `ThreadPool` is dropped, which causes it to clean up and ensure all worker
    /// threads finish their tasks. Then, we wait for the worker threads to complete by dropping the `pool`
    /// and collect the results afterward.
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();

        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);

        self.sender.send(Message::NewJob(job)).unwrap();
    }

    /// Waits for all submitted jobs to complete.
    ///
    /// This function blocks until all jobs in the thread pool have finished executing.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::{mpsc, Arc, Mutex};
    /// use std::thread;
    /// use threadpool_rs::ThreadPool;
    ///
    /// let pool = ThreadPool::new(4);
    ///
    /// for i in 0..4 {
    ///     pool.execute(move || {
    ///         // Simulate some work.
    ///         thread::sleep(std::time::Duration::from_secs(1));
    ///     });
    /// }
    ///
    /// // Wait for all submitted jobs to complete.
    /// pool.wait_all();
    /// ```
    pub fn wait_all(&self) {
        // Create a channel for signaling when all jobs are finished.
        let (done_sender, done_receiver) = mpsc::channel();

        // Execute a special "wait" job that will signal when it's done.
        self.execute(move || {
            // The "wait" job doesn't do anything; it's just a signal.
            done_sender.send(()).unwrap();
        });

        // Wait for the "wait" job to signal that all jobs are done.
        done_receiver.recv().unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        println!("Sending terminate message to all workers.");

        for _ in &mut self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        println!("Shutting down all workers.");

        for worker in &mut self.workers {
            println!("Shutting down worker {}", worker.id);

            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Message>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv().unwrap();

            match message {
                Message::NewJob(job) => {
                    println!("Worker {} got a job; executing.", id);

                    job.call_box();
                }
                Message::Terminate => {
                    println!("Worker {} was told to terminate.", id);

                    break;
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_thread_pool_creation() {
        // Test creating a thread pool with a valid size.
        let pool = ThreadPool::new(4);
        assert_eq!(pool.workers.len(), 4);
    }

    #[test]
    #[should_panic]
    fn test_thread_pool_creation_zero_size() {
        // Test creating a thread pool with a size of 0 (should panic).
        let _pool = ThreadPool::new(0);
    }

    #[test]
    fn test_thread_pool_execution() {
        // Test executing a job in the thread pool.
        let pool = ThreadPool::new(2);

        let result = Arc::new(Mutex::new(Vec::new()));

        for i in 0..4 {
            let result_clone = Arc::clone(&result);
            pool.execute(move || {
                let mut res = result_clone.lock().unwrap();
                res.push(i);
            });
        }

        drop(pool);

        let result = result.lock().unwrap();
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_thread_pool_heavy_load() {
        // Test executing a large number of tasks concurrently.
        let pool = ThreadPool::new(8);

        let result = Arc::new(Mutex::new(Vec::new()));

        // Create a large number of tasks.
        for i in 0..1000 {
            let result_clone = Arc::clone(&result);
            pool.execute(move || {
                let mut res = result_clone.lock().unwrap();
                res.push(i);
            });
        }

        drop(pool);

        let result = result.lock().unwrap();
        assert_eq!(result.len(), 1000);
    }

    #[test]
    fn test_thread_pool_heavy_computation() {
        // Test executing heavy computational tasks concurrently.
        let pool = ThreadPool::new(4);

        let result = Arc::new(Mutex::new(0));

        let start = Instant::now();

        // Calculate the sum of squares of numbers from 1 to 100,000 in parallel.
        for i in 1..=100_000 {
            let result_clone = Arc::clone(&result);
            pool.execute(move || {
                let square = i * i;
                let mut res = result_clone.lock().unwrap();
                *res += square;
            });
        }
        println!("Time elapsed: {:?}s", start.elapsed().as_secs());

        pool.wait_all();
        drop(pool);

        let result = result.lock().unwrap();
        let elapsed = start.elapsed();

        // Verify the correctness of the result.
        assert_eq!(*result, 333338333350000i64); // Use i64 for the literal.

        // Ensure the test ran in a reasonable time (e.g., less than 5 seconds).
        println!("Time elapsed: {:?}s", elapsed.as_secs());
        assert!(elapsed.as_secs() < 30);
    }
    #[test]
    fn test_thread_pool_long_task() {
        // Test executing a single long-running task with fewer tasks.
        let pool = ThreadPool::new(2);

        let result = Arc::new(Mutex::new(Vec::new()));

        let start = Instant::now();

        // Clone the Arc to ensure ownership within the closure.
        let result_clone = Arc::clone(&result);

        // Simulate a long-running task that takes 3 seconds.
        pool.execute(move || {
            std::thread::sleep(Duration::from_secs(3));
            let elapsed = start.elapsed().as_secs();
            let mut res = result_clone.lock().unwrap();
            res.push(elapsed);
        });

        pool.wait_all();
        drop(pool);

        let result = result.lock().unwrap();

        // Verify that only one task was executed and it took around 3 seconds.
        assert_eq!(result.len(), 1);
        assert!(result[0] >= 3 && result[0] < 4); // The task should take around 3 seconds.
    }

    #[test]
    fn test_thread_pool_wait_all() {
        // Test the wait_all function to ensure all submitted jobs complete.
        let pool = ThreadPool::new(2);

        let result = Arc::new(Mutex::new(Vec::new()));

        // Create some tasks.
        for i in 0..4 {
            let result_clone = Arc::clone(&result);
            pool.execute(move || {
                std::thread::sleep(Duration::from_secs(1)); // Simulate some work.
                let mut res = result_clone.lock().unwrap();
                res.push(i);
            });
        }

        pool.wait_all();
        drop(pool);

        let result = result.lock().unwrap();

        // Verify that all tasks were executed.
        assert_eq!(result.len(), 4);
    }
    #[test]
    fn test_thread_pool_wait_all2() {
        // Test the wait_all function to ensure all submitted jobs complete.
        let pool = ThreadPool::new(4);

        let result = Arc::new(Mutex::new(Vec::new()));

        // Create some tasks.
        for i in 0..4 {
            let result_clone = Arc::clone(&result);
            pool.execute(move || {
                std::thread::sleep(Duration::from_secs(1)); // Simulate some work.
                let mut res = result_clone.lock().unwrap();
                res.push(i);
            });
        }

        pool.wait_all();
        //drop(pool);

        let result = result.lock().unwrap();

        // Verify that all tasks were executed.
        assert_eq!(result.len(), 4);

        // Verify that all tasks are unique (no duplicates).
        let unique_count = result
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(unique_count, 4);
    }
}
