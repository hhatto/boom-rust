use std::collections::HashMap;
use std::iter;

#[derive(Debug, Default)]
pub struct Report {
    pub size_total: i64,
    pub size_per_req: i64,
    pub req_per_sec: f32,
    pub time_total: f32, // milliseconds
    pub time_exec_total: f32, // milliseconds
    pub time_average: f32, // milliseconds
    pub req_num: i32,
    pub results: Vec<(u16, f32)>, // (StatusCode, Milliseconds)
    pub status_num: HashMap<u16, i64>, // HashMap<StatusCode, OkCount>
    time_slowest: f32,
    time_fastest: f32,
    lats: Vec<i32>,
}

impl Report {
    pub fn new() -> Self {
        Report { ..Self::default() }
    }

    fn print(&mut self) {
        let (_, slowest) = self.results[self.results.len() - 1];
        let (_, fastest) = self.results[0];
        println!("Summary:");
        println!("  Total:        {:.5} s", self.time_exec_total / 1000.);
        println!("  Slowest:      {:.5} s", slowest / 1000.);
        println!("  Fastest:      {:.5} s", fastest / 1000.);
        println!("  Average:      {:.5} s", self.time_average / 1000.);
        println!("  Requests/sec: {:.2}", self.req_per_sec);
        println!("  Total data:   {:} bytes", self.size_total);
        println!("  Size/request: {:} bytes", self.size_per_req);
        println!("");

        for &(_, t) in self.results.iter() {
            self.lats.push(t as i32);
        }

        self.time_slowest = slowest;
        self.time_fastest = fastest;
        self.print_status();
        self.print_histogram();
        self.print_latency();
    }

    fn print_status(&mut self) {
        println!("Status code distribution:");
        for (k, v) in self.status_num.iter() {
            println!("  [{}] {} responses", k, v);
        }
        println!("");
    }

    fn print_histogram(&mut self) {
        let bc = 10;
        let mut buckets = vec![0.0; bc+1];
        let mut counts = vec![0; bc+1];
        let bs = (self.time_slowest - self.time_fastest) / bc as f32;

        for i in 0..bc {
            buckets[i] = self.time_fastest + bs * i as f32;
        }
        buckets[bc] = self.time_slowest;
        let mut bi = 0;
        let mut max = 0;
        let mut ri = 0;
        loop {
            if ri >= self.lats.len() {
                break;
            }
            if self.lats[ri] as f32 <= buckets[bi] {
                ri += 1;
                counts[bi] += 1;
                if max < counts[bi] {
                    max = counts[bi];
                }
            } else if bi < (buckets.len() - 1) {
                bi += 1;
            }
        }
        println!("Response time histogram:");
        for i in 0..buckets.len() {
            let mut bar_len = 0;
            if max > 0 {
                bar_len = counts[i] * 40 / max
            }
            println!("  {:-4.3} [{:-?}]\t|{}",
                     buckets[i] / 1000.,
                     counts[i],
                     iter::repeat("*").take(bar_len).collect::<String>());
        }
        println!("");
    }

    fn print_latency(&mut self) {
        let pctls = vec![10, 25, 50, 75, 90, 95, 99];
        let mut data = vec![0.0; pctls.len()];
        let mut j = 0;
        for i in 0..self.lats.len() {
            if !(i < self.lats.len() && j < pctls.len()) {
                break;
            }
            let current = i * 100 / self.lats.len();
            if current >= pctls[j] {
                data[j] = self.lats[i] as f32;
                j += 1;
            }
        }

        println!("Latency distribution:");
        for i in 0..pctls.len() {
            if data[i] > 0. {
                println!("  {}% in {:4.4} secs", pctls[i], data[i] / 1000.);
            }
        }
    }

    pub fn finalize(&mut self) {
        self.results.sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.time_average = self.time_total / self.req_num as f32;
        self.size_per_req = self.size_total / self.req_num as i64;
        self.print();
    }
}
