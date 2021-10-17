use std::collections::BTreeMap;
use std::iter;

use termcolor::{Color, ColorSpec, WriteColor};
use unicode_width::UnicodeWidthStr;

use crate::app::{DisplayConfig, RankingConfig, ValueFormat};
use crate::data;
use crate::Result;

#[derive(Clone, Debug)]
pub struct Comparisons {
    comps: Vec<Comparison>,
    config: DisplayConfig,
}

#[derive(Clone, Debug)]
pub struct Comparison {
    name: String,
    group: Option<String>,
    benchmarks: BTreeMap<String, Benchmark>,
    /// Mapping of fastest to slowest
    perf_ordered: Vec<String>,
    /// Mapping of command line order
    cmdline_ordered: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Benchmark {
    /// Baseline or group name
    name: String,
    nanoseconds: f64,
    stddev: Option<f64>,
    throughput: Option<data::Throughput>,
}

impl Comparisons {
    pub fn new(comps: Vec<Comparison>, config: DisplayConfig) -> Comparisons {
        Comparisons { comps, config }
    }

    pub fn is_empty(&self) -> bool {
        self.comps.is_empty()
    }

    pub fn drop_under(&mut self, threshold: f64) {
        self.comps.retain(|comp| comp.biggest_difference() > threshold);
    }

    fn throughput_available(&self) -> bool {
        self.comps
            .iter()
            .any(|c| c.benchmarks.iter().any(|(_n, b)| b.throughput.is_some()))
    }

    pub fn write(&self, mut wtr: Box<dyn WriteColor>) -> Result<()> {
        if self.config.list {
            self.rows(wtr.as_mut())?;
        } else {
            self.columns(wtr.as_mut())?;
        }
        wtr.flush()?;
        Ok(())
    }

    fn columns<W: WriteColor>(&self, mut wtr: W) -> Result<()> {
        let mut columns = Vec::new();
        for comp in &self.comps {
            for name in &comp.cmdline_ordered {
                if !columns.contains(name) {
                    columns.push(name.to_string());
                }
            }
        }

        write!(wtr, "group")?;
        for column in &columns {
            write!(wtr, "\t  {}", column)?;
        }
        writeln!(wtr, "")?;

        write_divider(&mut wtr, '-', "group".width())?;
        for column in &columns {
            write!(wtr, "\t  ")?;
            write_divider(&mut wtr, '-', column.width())?;
        }
        writeln!(wtr, "")?;

        let throughput_available = self.throughput_available();
        for comp in &self.comps {
            if comp.benchmarks.is_empty() {
                continue;
            }

            write!(wtr, "{}", comp.name)?;
            let divide_by = self.divide_by(&comp);
            for column_name in &columns {
                let b = match comp.get(column_name) {
                    Some(b) => b,
                    None => {
                        write!(wtr, "\t")?;
                        continue;
                    }
                };
                let val = b.nanoseconds / divide_by;
                let (val, sign) = match self.config.value_format {
                    ValueFormat::Percent => (val * 100.0, "%"),
                    ValueFormat::Real => (val, ""),
                };
                let color_set =
                    self.set_color(&mut wtr, comp, b.nanoseconds)?;
                if throughput_available {
                    write!(
                        wtr,
                        "\t  {:>8.2}{} {:>14} {:>14}",
                        val,
                        sign,
                        time(b.nanoseconds, b.stddev),
                        throughput(b.throughput),
                    )?;
                } else {
                    write!(
                        wtr,
                        "\t  {:>8.2}{} {:>14}",
                        val,
                        sign,
                        time(b.nanoseconds, b.stddev),
                    )?;
                }
                if color_set {
                    wtr.reset()?;
                }
            }
            writeln!(wtr, "")?;
        }
        Ok(())
    }

    fn set_color<W: WriteColor>(
        &self,
        wtr: &mut W,
        comp: &Comparison,
        current: f64,
    ) -> Result<bool> {
        let color_conf = match self.config.rank {
            RankingConfig::Baseline => {
                self.set_colors_baseline_mode(comp, current)
            }
            RankingConfig::Benchmark => {
                self.set_colors_benchmark_mode(comp, current)
            }
        };
        match color_conf {
            Some((color, bold)) => {
                let mut spec = ColorSpec::new();
                spec.set_fg(Some(color)).set_bold(bold);
                wtr.set_color(&spec)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn set_colors_benchmark_mode(
        &self,
        comp: &Comparison,
        current: f64,
    ) -> Option<(Color, bool)> {
        let best = comp.best().unwrap().nanoseconds;
        if best == current {
            Some((Color::Green, true))
        } else {
            None
        }
    }

    fn set_colors_baseline_mode(
        &self,
        comp: &Comparison,
        current: f64,
    ) -> Option<(Color, bool)> {
        const THRESHOLD1: f64 = 0.03;
        const THRESHOLD2: f64 = 0.1;
        let first = comp.first().unwrap().nanoseconds;
        let val = current / first;
        let diff = val - 1.0;
        if diff > 0.0 {
            if diff < THRESHOLD1 {
                return None;
            } else if diff < THRESHOLD2 {
                return Some((Color::Red, false));
            } else {
                return Some((Color::Red, true));
            }
        } else {
            let diff = -diff;
            if diff < THRESHOLD1 {
                return None;
            } else if diff < THRESHOLD2 {
                return Some((Color::Green, false));
            } else {
                return Some((Color::Green, true));
            }
        }
    }

    fn rows<W: WriteColor>(&self, mut wtr: W) -> Result<()> {
        for (i, comp) in self.comps.iter().enumerate() {
            if i > 0 {
                writeln!(wtr, "")?;
            }
            self.rows_one(&mut wtr, comp)?;
        }
        Ok(())
    }

    fn rows_one<W: WriteColor>(
        &self,
        mut wtr: W,
        comp: &Comparison,
    ) -> Result<()> {
        writeln!(wtr, "{}", comp.name)?;
        write_divider(&mut wtr, '-', comp.name.width())?;
        writeln!(wtr, "")?;

        if comp.benchmarks.is_empty() {
            writeln!(wtr, "NOTHING TO SHOW")?;
            return Ok(());
        }

        let divide_by = self.divide_by(comp);
        for b in comp.benchmarks.values() {
            let val = b.nanoseconds / divide_by;
            let (val, sign) = match self.config.value_format {
                ValueFormat::Percent => (val * 100.0, "%"),
                ValueFormat::Real => (val, ""),
            };
            writeln!(
                wtr,
                "{}\t{:>7.2}{}\t{:>15}\t{:>12}",
                b.name,
                val,
                sign,
                time(b.nanoseconds, b.stddev),
                throughput(b.throughput),
            )?;
        }
        Ok(())
    }

    fn divide_by(&self, comp: &Comparison) -> f64 {
        match self.config.rank {
            RankingConfig::Benchmark => comp.best().unwrap().nanoseconds,
            RankingConfig::Baseline => comp.first().unwrap().nanoseconds,
        }
    }
}

impl Comparison {
    pub fn new(name: &str, mut benchmarks: Vec<Benchmark>) -> Comparison {
        // name will be either in the `<name>` format or `<group>/<name>` format
        let name_parts: Vec<_> = name.split('/').collect();
        let (group, name) = if name_parts.len() >= 2 {
            (Some(name_parts[0].into()), name_parts[1].into())
        } else {
            (None, name_parts[0].into())
        };
        let mut comp = Comparison {
            name,
            group,
            benchmarks: benchmarks
                .clone()
                .into_iter()
                .map(|b| (b.name.clone(), b))
                .collect(),
            perf_ordered: Vec::new(),
            cmdline_ordered: Vec::new(),
        };
        if comp.benchmarks.is_empty() {
            return comp;
        }
        comp.cmdline_ordered =
            benchmarks.iter().map(|b| b.name.clone()).collect();
        benchmarks.sort_by(|a, b| {
            a.nanoseconds.partial_cmp(&b.nanoseconds).unwrap()
        });
        comp.perf_ordered = benchmarks.into_iter().map(|b| b.name).collect();
        comp
    }

    /// Return the biggest difference, percentage wise, between benchmarks
    /// in this comparison.
    ///
    /// If this comparison has fewer than two benchmarks, then 0 is returned.
    pub fn biggest_difference(&self) -> f64 {
        if self.benchmarks.len() < 2 {
            return 0.0;
        }
        let best = self.best().unwrap().nanoseconds;
        let worst = self.worst().unwrap().nanoseconds;
        ((worst - best) / best) * 100.0
    }

    fn best(&self) -> Option<&Benchmark> {
        self.get_by_perf(0)
    }

    fn worst(&self) -> Option<&Benchmark> {
        self.get_by_perf(self.perf_ordered.len() - 1)
    }

    fn get_by_perf(&self, pos: usize) -> Option<&Benchmark> {
        self.perf_ordered.get(pos).and_then(|n| self.benchmarks.get(n))
    }

    fn get_by_cmdline(&self, pos: usize) -> Option<&Benchmark> {
        self.cmdline_ordered.get(pos).and_then(|n| self.benchmarks.get(n))
    }

    /// The baseline/group which was specified first on the commandline
    fn first(&self) -> Option<&Benchmark> {
        self.get_by_cmdline(0)
    }

    fn get(&self, name: &str) -> Option<&Benchmark> {
        self.benchmarks.get(name)
    }
}

impl Benchmark {
    pub fn from_data(b: &data::Benchmark) -> Benchmark {
        Benchmark {
            name: b.fullname().to_string(),
            nanoseconds: b.nanoseconds(),
            stddev: Some(b.stddev()),
            throughput: b.throughput(),
        }
    }

    pub fn name(self, name: &str) -> Benchmark {
        Benchmark { name: name.to_string(), ..self }
    }
}

fn write_divider<W: WriteColor>(
    mut wtr: W,
    divider: char,
    width: usize,
) -> Result<()> {
    let div: String = iter::repeat(divider).take(width).collect();
    write!(wtr, "{}", div)?;
    Ok(())
}

fn time(nanos: f64, stddev: Option<f64>) -> String {
    const MIN_MICRO: f64 = 2_000.0;
    const MIN_MILLI: f64 = 2_000_000.0;
    const MIN_SEC: f64 = 2_000_000_000.0;

    let (div, label) = if nanos < MIN_MICRO {
        (1.0, "ns")
    } else if nanos < MIN_MILLI {
        (1_000.0, "µs")
    } else if nanos < MIN_SEC {
        (1_000_000.0, "ms")
    } else {
        (1_000_000_000.0, "s")
    };
    if let Some(stddev) = stddev {
        format!("{:.1}±{:.2}{}", nanos / div, stddev / div, label)
    } else {
        format!("{:.1}{}", nanos / div, label)
    }
}

fn throughput(throughput: Option<data::Throughput>) -> String {
    use data::Throughput::*;
    match throughput {
        Some(Bytes(num)) => throughput_per(num, "B"),
        Some(Elements(num)) => throughput_per(num, "Elem"),
        _ => "? ?/sec".to_string(),
    }
}

fn throughput_per(per: f64, unit: &str) -> String {
    const MIN_K: f64 = (2 * (1 << 10) as u64) as f64;
    const MIN_M: f64 = (2 * (1 << 20) as u64) as f64;
    const MIN_G: f64 = (2 * (1 << 30) as u64) as f64;

    if per < MIN_K {
        format!("{} {}/sec", per as u64, unit)
    } else if per < MIN_M {
        format!("{:.1} K{}/sec", (per / (1 << 10) as f64), unit)
    } else if per < MIN_G {
        format!("{:.1} M{}/sec", (per / (1 << 20) as f64), unit)
    } else {
        format!("{:.1} G{}/sec", (per / (1 << 30) as f64), unit)
    }
}
