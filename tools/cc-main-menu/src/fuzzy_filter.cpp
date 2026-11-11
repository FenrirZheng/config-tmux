#include "fuzzy_filter.hpp"

#include <algorithm>
#include <string>
#include <utility>

namespace ccmm {

namespace {

char lower(char c) { return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c; }

bool boundary(std::string_view s, std::size_t i) {
  if (i == 0) return true;
  const char p = s[i - 1];
  return p == '-' || p == ' ' || p == '_' || p == '/' || p == ':' || p == '(';
}

std::string to_lower(std::string_view s) {
  std::string out(s);
  for (auto& c : out) c = lower(c);
  return out;
}

int clamp_len(std::size_t n) { return static_cast<int>(std::min<std::size_t>(n, 200)); }

std::vector<std::string_view> split_terms(std::string_view q) {
  std::vector<std::string_view> out;
  while (!q.empty()) {
    const auto s = q.find_first_not_of(' ');
    if (s == std::string_view::npos) break;
    q.remove_prefix(s);
    const auto e = q.find(' ');
    out.push_back(q.substr(0, e));
    if (e == std::string_view::npos) break;
    q.remove_prefix(e);
  }
  return out;
}

}  // namespace

std::optional<int> score_term(std::string_view term, std::string_view field) {
  if (term.empty()) return 0;
  if (field.empty() || term.size() > field.size()) return std::nullopt;
  const std::string t = to_lower(term);
  const std::string f = to_lower(field);
  const int extra = clamp_len(f.size() - t.size());

  if (f == t) return 1000;
  if (f.compare(0, t.size(), t) == 0) return 800 - extra;

  int best_sub = -1;
  for (auto pos = f.find(t); pos != std::string::npos; pos = f.find(t, pos + 1)) {
    const int s = (boundary(f, pos) ? 600 : 450) - clamp_len(pos) - extra / 4;
    best_sub = std::max(best_sub, s);
  }
  if (best_sub >= 0) return best_sub;

  // Subsequence, greedy left-to-right, rewarding runs and word starts.
  int score = 100;
  std::size_t fi = 0;
  std::size_t last = std::string::npos;
  for (const char c : t) {
    while (fi < f.size() && f[fi] != c) ++fi;
    if (fi == f.size()) return std::nullopt;
    if (last != std::string::npos && fi == last + 1) score += 15;
    if (boundary(f, fi)) score += 10;
    if (last != std::string::npos) score -= clamp_len(fi - last - 1);
    last = fi++;
  }
  return std::max(score - extra / 4, 1);
}

std::optional<int> score_action(std::string_view query, const Action& action) {
  const auto terms = split_terms(query);
  if (terms.empty()) return 0;

  // Weighted fields: the primary label counts fully, secondary text less, so
  // "split" ranks split-window above a key whose bound command says split.
  std::vector<std::pair<std::string_view, int>> fields;  // field, percent
  if (action.kind == Kind::Command) {
    fields.emplace_back(action.name, 100);
    fields.emplace_back(action.alias, 95);
  } else {
    fields.emplace_back(action.note, 100);
    fields.emplace_back(action.bound_command, 60);
  }
  // A noted key's badge is its identity, so it counts fully there; on a command
  // it is secondary — "prefix F11" should find the key, not what it runs.
  const int badge_pct = action.kind == Kind::Binding ? 100 : 70;
  for (const auto& b : action.badges) fields.emplace_back(b, badge_pct);

  int total = 0;
  for (const auto term : terms) {
    int best = -1;
    for (const auto& [field, pct] : fields) {
      if (const auto s = score_term(term, field)) best = std::max(best, *s * pct / 100);
    }
    if (best < 0) return std::nullopt;
    total += best;
  }
  return total;
}

std::vector<std::size_t> filter(std::string_view query, const std::vector<Action>& actions) {
  std::vector<std::pair<int, std::size_t>> hits;
  for (std::size_t i = 0; i < actions.size(); ++i)
    if (const auto s = score_action(query, actions[i])) hits.emplace_back(*s, i);
  std::stable_sort(hits.begin(), hits.end(),
                   [](const auto& a, const auto& b) { return a.first > b.first; });
  std::vector<std::size_t> out;
  out.reserve(hits.size());
  for (const auto& h : hits) out.push_back(h.second);
  return out;
}

}  // namespace ccmm
