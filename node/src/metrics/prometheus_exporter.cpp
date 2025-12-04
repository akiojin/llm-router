#include "metrics/prometheus_exporter.h"

#include <sstream>
#include <algorithm>

namespace llm_node::metrics {

void PrometheusExporter::set_gauge(const std::string& name, double value, const std::string& help) {
    std::lock_guard<std::mutex> lk(mu_);
    auto it = std::find_if(gauges_.begin(), gauges_.end(), [&](const Gauge& g) { return g.name == name; });
    if (it == gauges_.end()) {
        gauges_.push_back(Gauge{name, help, value});
    } else {
        it->value = value;
        if (!help.empty()) it->help = help;
    }
}

void PrometheusExporter::inc_counter(const std::string& name, double delta, const std::string& help) {
    std::lock_guard<std::mutex> lk(mu_);
    auto it = std::find_if(counters_.begin(), counters_.end(), [&](const Counter& c) { return c.name == name; });
    if (it == counters_.end()) {
        counters_.push_back(Counter{name, help, delta});
    } else {
        it->value += delta;
        if (!help.empty()) it->help = help;
    }
}

static void write_help(std::ostringstream& oss, const std::string& name, const std::string& help) {
    if (!help.empty()) {
        oss << "# HELP " << name << " " << help << "\n";
    }
    oss << "# TYPE " << name << " ";
}

std::string PrometheusExporter::render() const {
    std::lock_guard<std::mutex> lk(mu_);
    std::ostringstream oss;
    for (const auto& g : gauges_) {
        write_help(oss, g.name, g.help);
        oss << "gauge\n" << g.name << " " << g.value << "\n";
    }
    for (const auto& c : counters_) {
        write_help(oss, c.name, c.help);
        oss << "counter\n" << c.name << " " << c.value << "\n";
    }
    return oss.str();
}

}  // namespace llm_node::metrics
