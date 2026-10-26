use serde::{Deserialize, Serialize};

/// One pile of identical miniatures stored in a single location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockMini {
    pub id: i64,
    pub name: String,
    pub quantity: u32,
    pub size: String,
    pub game: Option<String>,
    pub location_id: i64,
    pub location_path: String,
    pub races: Vec<String>,
    pub roles: Vec<String>,
    pub combat: Vec<String>,
    /// Race, role, and combat names together, for a generic tag match.
    pub tags: Vec<String>,
}

/// One thing the encounter generator wants pulled from storage.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Need {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub races: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub combat: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub game: Option<String>,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pick {
    pub request_index: usize,
    pub label: String,
    pub miniature_id: i64,
    pub name: String,
    pub quantity: u32,
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocationGroup {
    pub location_id: i64,
    pub location_path: String,
    pub picks: Vec<Pick>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestResult {
    pub request_index: usize,
    pub label: String,
    pub wanted: u32,
    pub allocated: u32,
    pub short: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shortage {
    pub request_index: usize,
    pub label: String,
    pub wanted: u32,
    pub available: u32,
    pub short: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrepList {
    pub groups: Vec<LocationGroup>,
    pub requests: Vec<RequestResult>,
}

impl PrepList {
    pub fn shortages(&self) -> Vec<Shortage> {
        self.requests
            .iter()
            .filter(|request| request.short > 0)
            .map(|request| Shortage {
                request_index: request.request_index,
                label: request.label.clone(),
                wanted: request.wanted,
                available: request.allocated,
                short: request.short,
            })
            .collect()
    }
}

/// True when the need names at least one constraint besides quantity.
pub fn need_has_criteria(need: &Need) -> bool {
    let named = need
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some();
    named
        || !blank(&need.races)
        || !blank(&need.roles)
        || !blank(&need.combat)
        || !blank(&need.tags)
        || need
            .size
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        || need
            .game
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
}

fn blank(values: &[String]) -> bool {
    values.iter().all(|value| value.trim().is_empty())
}

/// Case-insensitive word-prefix match. "orc" matches "Orc Warrior" and
/// does not match "Sorcerer". "wiz" matches "Wizard".
pub fn name_matches(name: &str, query: &str) -> bool {
    let query_words = words(query);
    if query_words.is_empty() {
        return true;
    }
    let name_words = words(name);
    query_words.iter().all(|word| {
        name_words
            .iter()
            .any(|name_word| name_word.starts_with(word))
    })
}

fn words(value: &str) -> Vec<String> {
    value
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect()
}

fn matches(mini: &StockMini, need: &Need) -> bool {
    if let Some(name) = need.name.as_deref() {
        if !name_matches(&mini.name, name) {
            return false;
        }
    }
    if !eq_ignore(&mini.size, need.size.as_deref()) {
        return false;
    }
    if !eq_ignore_opt(mini.game.as_deref(), need.game.as_deref()) {
        return false;
    }
    facet_matches(&mini.races, &need.races)
        && facet_matches(&mini.roles, &need.roles)
        && facet_matches(&mini.combat, &need.combat)
        && facet_matches(&mini.tags, &need.tags)
}

fn eq_ignore(have: &str, want: Option<&str>) -> bool {
    match want.map(str::trim).filter(|s| !s.is_empty()) {
        None => true,
        Some(want) => have.trim().eq_ignore_ascii_case(want),
    }
}

fn eq_ignore_opt(have: Option<&str>, want: Option<&str>) -> bool {
    match want.map(str::trim).filter(|s| !s.is_empty()) {
        None => true,
        Some(want) => have
            .map(str::trim)
            .is_some_and(|have| have.eq_ignore_ascii_case(want)),
    }
}

pub fn facet_matches(have: &[String], want: &[String]) -> bool {
    let want: Vec<String> = want
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect();
    if want.is_empty() {
        return true;
    }
    let have: Vec<String> = have
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect();
    want.iter()
        .any(|wanted| have.iter().any(|owned| owned == wanted))
}

/// Fill needs in the order given. Earlier needs keep the miniatures they take.
/// Picks are grouped by storage location so they can be pulled in one walk.
pub fn build_prep_list(stock: &[StockMini], needs: &[Need]) -> PrepList {
    let mut remaining: Vec<u32> = stock.iter().map(|mini| mini.quantity).collect();
    let mut allocations: Vec<(i64, String, Pick)> = Vec::new();
    let mut requests = Vec::with_capacity(needs.len());

    for (request_index, need) in needs.iter().enumerate() {
        let mut still = need.quantity;
        let mut allocated = 0u32;
        let mut candidates: Vec<usize> = (0..stock.len())
            .filter(|&index| remaining[index] > 0 && matches(&stock[index], need))
            .collect();
        candidates.sort_by(|&left, &right| {
            stock[left]
                .location_path
                .cmp(&stock[right].location_path)
                .then(stock[left].id.cmp(&stock[right].id))
        });
        for index in candidates {
            if still == 0 {
                break;
            }
            let take = remaining[index].min(still);
            remaining[index] -= take;
            still -= take;
            allocated += take;
            let mini = &stock[index];
            allocations.push((
                mini.location_id,
                mini.location_path.clone(),
                Pick {
                    request_index,
                    label: need.label.clone(),
                    miniature_id: mini.id,
                    name: mini.name.clone(),
                    quantity: take,
                    size: mini.size.clone(),
                },
            ));
        }
        let wanted = need.quantity;
        requests.push(RequestResult {
            request_index,
            label: need.label.clone(),
            wanted,
            allocated,
            short: wanted.saturating_sub(allocated),
        });
    }

    let mut groups: Vec<LocationGroup> = Vec::new();
    for (location_id, location_path, pick) in allocations {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.location_id == location_id)
        {
            group.picks.push(pick);
        } else {
            groups.push(LocationGroup {
                location_id,
                location_path,
                picks: vec![pick],
            });
        }
    }
    groups.sort_by(|left, right| {
        left.location_path
            .cmp(&right.location_path)
            .then(left.location_id.cmp(&right.location_id))
    });

    PrepList { groups, requests }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn mini(
        id: i64,
        name: &str,
        quantity: u32,
        size: &str,
        game: Option<&str>,
        location_id: i64,
        path: &str,
        races: &[&str],
        roles: &[&str],
        combat: &[&str],
    ) -> StockMini {
        let mut tags = Vec::new();
        tags.extend(races.iter().map(|s| (*s).to_string()));
        tags.extend(roles.iter().map(|s| (*s).to_string()));
        tags.extend(combat.iter().map(|s| (*s).to_string()));
        StockMini {
            id,
            name: name.to_string(),
            quantity,
            size: size.to_string(),
            game: game.map(str::to_string),
            location_id,
            location_path: path.to_string(),
            races: races.iter().map(|s| (*s).to_string()).collect(),
            roles: roles.iter().map(|s| (*s).to_string()).collect(),
            combat: combat.iter().map(|s| (*s).to_string()).collect(),
            tags,
        }
    }

    fn need(label: &str, quantity: u32) -> Need {
        Need {
            label: label.to_string(),
            name: None,
            races: vec![],
            roles: vec![],
            combat: vec![],
            tags: vec![],
            size: None,
            game: None,
            quantity,
        }
    }

    #[test]
    fn allocates_a_full_match_from_one_location() {
        let stock = vec![mini(
            1,
            "Orc Warrior",
            4,
            "28mm",
            Some("Massive Darkness"),
            10,
            "Garage > Cabinet A > Drawer 3 > Box 2",
            &["orc"],
            &["barbarian"],
            &["melee"],
        )];
        let mut request = need("front line", 3);
        request.races = vec!["orc".into()];
        request.combat = vec!["melee".into()];

        let list = build_prep_list(&stock, &[request]);

        assert!(list.shortages().is_empty());
        assert_eq!(list.groups.len(), 1);
        assert_eq!(
            list.groups[0].location_path,
            "Garage > Cabinet A > Drawer 3 > Box 2"
        );
        assert_eq!(list.groups[0].picks.len(), 1);
        assert_eq!(list.groups[0].picks[0].miniature_id, 1);
        assert_eq!(list.groups[0].picks[0].quantity, 3);
        assert_eq!(list.requests[0].allocated, 3);
        assert_eq!(list.requests[0].short, 0);
    }

    #[test]
    fn splits_one_need_across_locations_and_groups_the_picks() {
        let stock = vec![
            mini(
                2,
                "Goblin Spearman",
                2,
                "28mm",
                None,
                3,
                "Office > Shelf > Container 1",
                &["goblin"],
                &["warrior"],
                &["melee"],
            ),
            mini(
                1,
                "Goblin Spearman",
                3,
                "28mm",
                None,
                2,
                "Garage > Cabinet A > Drawer 3 > Box 2",
                &["goblin"],
                &["warrior"],
                &["melee"],
            ),
        ];
        let mut request = need("goblin screen", 4);
        request.races = vec!["goblin".into()];

        let list = build_prep_list(&stock, &[request]);

        assert!(list.shortages().is_empty());
        assert_eq!(list.groups.len(), 2);
        assert_eq!(
            list.groups[0].location_path,
            "Garage > Cabinet A > Drawer 3 > Box 2"
        );
        assert_eq!(list.groups[0].picks[0].quantity, 3);
        assert_eq!(list.groups[1].location_path, "Office > Shelf > Container 1");
        assert_eq!(list.groups[1].picks[0].quantity, 1);
        assert_eq!(list.groups[1].picks[0].miniature_id, 2);
    }

    #[test]
    fn reports_a_shortage_when_quantity_is_short() {
        let stock = vec![mini(
            1,
            "Human Wizard",
            1,
            "32mm",
            Some("Descent"),
            8,
            "Office > Shelf > Container 1",
            &["human"],
            &["wizard", "mage"],
            &["magic"],
        )];
        let mut request = need("casters", 3);
        request.roles = vec!["wizard".into()];

        let list = build_prep_list(&stock, &[request]);

        assert_eq!(list.requests[0].wanted, 3);
        assert_eq!(list.requests[0].allocated, 1);
        assert_eq!(list.requests[0].short, 2);
        assert_eq!(list.shortages().len(), 1);
        assert_eq!(list.shortages()[0].label, "casters");
        assert_eq!(list.groups[0].picks[0].quantity, 1);
    }

    #[test]
    fn reports_the_full_need_when_nothing_matches() {
        let stock = vec![mini(
            1,
            "Orc Warrior",
            4,
            "28mm",
            None,
            1,
            "Garage",
            &["orc"],
            &["barbarian"],
            &["melee"],
        )];
        let mut request = need("elf archers", 5);
        request.races = vec!["elf".into()];
        request.combat = vec!["ranged".into()];

        let list = build_prep_list(&stock, &[request]);

        assert!(list.groups.is_empty());
        assert_eq!(list.requests[0].allocated, 0);
        assert_eq!(list.requests[0].short, 5);
        assert_eq!(list.shortages()[0].available, 0);
    }

    #[test]
    fn facets_combine_with_and_and_values_inside_a_facet_with_or() {
        let stock = vec![
            mini(
                1,
                "Orc Warrior",
                4,
                "28mm",
                None,
                1,
                "Garage > Box 2",
                &["orc"],
                &["barbarian"],
                &["melee"],
            ),
            mini(
                2,
                "Human Wizard",
                1,
                "32mm",
                Some("Descent"),
                2,
                "Office > Container 1",
                &["human"],
                &["wizard", "mage", "sorcerer"],
                &["magic"],
            ),
        ];
        let mut wizard_or_mage = need("arcane", 1);
        wizard_or_mage.roles = vec!["sorcerer".into(), "witch".into()];
        let mut orc_wizard = need("orc wizard", 1);
        orc_wizard.races = vec!["orc".into()];
        orc_wizard.roles = vec!["wizard".into()];

        let list = build_prep_list(&stock, &[wizard_or_mage, orc_wizard]);

        assert_eq!(list.requests[0].allocated, 1);
        assert_eq!(list.groups[0].picks[0].miniature_id, 2);
        assert_eq!(list.requests[1].allocated, 0);
        assert_eq!(list.requests[1].short, 1);
    }

    #[test]
    fn name_matches_whole_words_and_prefixes_not_substrings_inside_words() {
        let stock = vec![
            mini(
                1,
                "Human Wizard",
                1,
                "32mm",
                None,
                1,
                "Office",
                &["human"],
                &["wizard"],
                &["magic"],
            ),
            mini(
                2,
                "Orc Warrior",
                2,
                "28mm",
                None,
                1,
                "Office",
                &["orc"],
                &["barbarian"],
                &["melee"],
            ),
        ];
        let mut wiz = need("wiz", 1);
        wiz.name = Some("wiz".into());
        let mut orc = need("orc", 1);
        orc.name = Some("orc".into());

        let list = build_prep_list(&stock, &[wiz, orc]);

        assert_eq!(list.requests[0].allocated, 1);
        assert_eq!(
            list.groups
                .iter()
                .flat_map(|g| &g.picks)
                .find(|p| p.request_index == 0)
                .unwrap()
                .miniature_id,
            1
        );
        assert_eq!(list.requests[1].allocated, 1);
        assert_eq!(
            list.groups
                .iter()
                .flat_map(|g| &g.picks)
                .find(|p| p.request_index == 1)
                .unwrap()
                .miniature_id,
            2
        );
    }

    #[test]
    fn name_query_orc_does_not_match_sorcerer() {
        let stock = vec![mini(
            1,
            "Sorcerer",
            1,
            "32mm",
            None,
            1,
            "Office",
            &["human"],
            &["sorcerer"],
            &["magic"],
        )];
        let mut request = need("orcs", 1);
        request.name = Some("orc".into());

        let list = build_prep_list(&stock, &[request]);

        assert_eq!(list.requests[0].allocated, 0);
        assert_eq!(list.requests[0].short, 1);
    }

    #[test]
    fn size_and_game_are_exact_and_an_unset_game_does_not_match_a_named_game() {
        let stock = vec![
            mini(
                1,
                "A",
                2,
                "28mm",
                None,
                1,
                "Garage",
                &["orc"],
                &[],
                &["melee"],
            ),
            mini(
                2,
                "B",
                2,
                "28mm",
                Some("Descent"),
                1,
                "Garage",
                &["orc"],
                &[],
                &["melee"],
            ),
        ];
        let mut sized = need("28s", 1);
        sized.size = Some("28MM".into());
        sized.races = vec!["orc".into()];
        let mut game = need("descent orcs", 1);
        game.game = Some("descent".into());
        game.races = vec!["orc".into()];

        let list = build_prep_list(&stock, &[game, sized]);

        assert_eq!(list.requests[0].allocated, 1);
        let game_pick = list
            .groups
            .iter()
            .flat_map(|g| &g.picks)
            .find(|p| p.request_index == 0)
            .unwrap();
        assert_eq!(game_pick.miniature_id, 2);
        assert_eq!(list.requests[1].allocated, 1);
    }

    #[test]
    fn later_requests_only_get_what_earlier_requests_left() {
        let stock = vec![mini(
            1,
            "Orc Warrior",
            5,
            "28mm",
            None,
            4,
            "Garage > Box 2",
            &["orc"],
            &["barbarian"],
            &["melee"],
        )];
        let mut first = need("encounter A", 3);
        first.races = vec!["orc".into()];
        let mut second = need("encounter B", 4);
        second.races = vec!["orc".into()];

        let list = build_prep_list(&stock, &[first, second]);

        assert_eq!(list.requests[0].allocated, 3);
        assert_eq!(list.requests[0].short, 0);
        assert_eq!(list.requests[1].allocated, 2);
        assert_eq!(list.requests[1].short, 2);
        assert_eq!(list.groups.len(), 1);
        assert_eq!(list.groups[0].picks.len(), 2);
        assert_eq!(list.groups[0].picks[0].quantity, 3);
        assert_eq!(list.groups[0].picks[1].quantity, 2);
    }

    #[test]
    fn zero_quantity_stock_is_not_used() {
        let stock = vec![mini(
            1,
            "Elf Scout",
            0,
            "32mm",
            None,
            1,
            "Office",
            &["elf"],
            &["ranger"],
            &["ranged"],
        )];
        let mut request = need("scouts", 1);
        request.races = vec!["elf".into()];

        let list = build_prep_list(&stock, &[request]);

        assert!(list.groups.is_empty());
        assert_eq!(list.requests[0].short, 1);
    }

    #[test]
    fn generic_tags_match_race_role_or_combat() {
        let stock = vec![mini(
            1,
            "Human Wizard",
            1,
            "32mm",
            None,
            1,
            "Office",
            &["human"],
            &["wizard"],
            &["magic"],
        )];
        let mut request = need("by tag", 1);
        request.tags = vec!["magic".into()];

        let list = build_prep_list(&stock, &[request]);

        assert_eq!(list.requests[0].allocated, 1);
    }

    #[test]
    fn empty_need_list_returns_no_groups_and_no_shortages() {
        let stock = vec![mini(
            1,
            "Orc Warrior",
            4,
            "28mm",
            None,
            1,
            "Garage",
            &["orc"],
            &[],
            &["melee"],
        )];
        let list = build_prep_list(&stock, &[]);
        assert!(list.groups.is_empty());
        assert!(list.requests.is_empty());
        assert!(list.shortages().is_empty());
    }

    #[test]
    fn name_match_is_case_insensitive_on_each_word() {
        assert!(name_matches("Orc Warrior", "ORC warrior"));
        assert!(name_matches("Human Wizard", "wiz"));
        assert!(!name_matches("Sorcerer", "orc"));
        assert!(name_matches("Skeleton Warrior", ""));
    }
}
