// prometheus_exporter.h - minimal Prometheus text exporter for llm-node
#pragma once

#include <string>
#include <vector>
#include <mutex>

namespace llm_node::metrics {

struct Gauge {
    std::string name;
    std::string help;
    double value;
};

struct Counter {
    std::string name;
    std::string help;
    double value;
};

class PrometheusExporter {
  public:
    void set_gauge(const std::string& name, double value, const std::string& help = "");
    void inc_counter(const std::string& name, double delta = 1.0, const std::string& help = "");
    std::string render() const;

  private:
    mutable std::mutex mu_;
    std::vector<Gauge> gauges_;
    std::vector<Counter> counters_;
};

}  // namespace llm_node::metrics
