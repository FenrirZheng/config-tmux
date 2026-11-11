// fuzzy_filter — rank actions against the query typed in the search bar.
//
// A query is split on spaces into terms; every term must match some field of
// the action (name, alias, note, key badge, bound command) and the action's
// score is the sum of each term's best field score. Case-insensitive, ASCII.
#pragma once

#include <optional>
#include <string_view>
#include <vector>

#include "tmux_client.hpp"

namespace ccmm {

// Score of one term against one field: exact > prefix > word-boundary substring
// > substring > subsequence. nullopt = no match.
std::optional<int> score_term(std::string_view term, std::string_view field);

// Score of a whole query against an action; nullopt = filtered out. An empty
// query matches everything with score 0.
std::optional<int> score_action(std::string_view query, const Action& action);

// Indices into `actions` that match, best score first; ties keep catalog order.
std::vector<std::size_t> filter(std::string_view query, const std::vector<Action>& actions);

}  // namespace ccmm
