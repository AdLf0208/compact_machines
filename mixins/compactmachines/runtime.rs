// Compact Machines slice 1: the pure rules of a machine room.
//
// Everything here is game-free: the jar's templates and sizes, room codes, the
// 1024-block region spiral, a room's shell geometry, and the store of
// rooms/machines/player history with its world file. The glue in `support.rs`
// reads the world and calls these functions; no game type appears in this file.
//
// Sizes, colours and the code alphabet come from the jar:
// `data/.../room_templates/*.json` and `RoomCodeGenerator.java` (FINDINGS.md
// §2). Nothing here is invented.

use std::collections::BTreeMap;

/// The room-code alphabet: the digits and the 21 letters
/// `RoomCodeGenerator.java` keeps (`0123456789ABDFGHJKLMNPQRSTVWXYZ`, i.e. no
/// `C`, `E`, `I`, `O` or `U`). 31 characters, 12 drawn per code (~59 bits).
pub const ALPHABET: &str = "0123456789ABDFGHJKLMNPQRSTVWXYZ";

/// Where the reserved room volume starts: the interior floor's Y and the block
/// centre of region (0, 0). Rooms are sealed shells here because the pin has
/// no dimension API (FINDINGS.md §4).
pub const ROOM_BASE: [i32; 3] = [30_000, 200, 30_000];

/// The blocks one room region spans: 64 chunks (`MathUtil.getRegionPositionByIndex`
/// feeds `ChunkPos(region * 64)`).
pub const REGION_BLOCKS: i32 = 64 * 16;

/// The item components a machine carries (`CMDataComponents`, the jar's
/// `compactmachines:` names).
pub const ROOM_TEMPLATE_COMPONENT: &str = "compactmachines:room_template";
pub const ROOM_CODE_COMPONENT: &str = "compactmachines:room_code";
pub const MACHINE_COLOR_COMPONENT: &str = "compactmachines:machine_color";

/// `ShrinkingDeviceConfiguration.ALLOWED_DEPTH_DEFAULT` (the PSD's default).
pub const MAX_ENTRY_DEPTH: usize = 5;

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// The room templates the jar's `basic_templates` datapack defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Template {
    Tiny,
    Small,
    Normal,
    Large,
    Giant,
    Colossal,
    Farming,
    Soaryn,
}

impl Template {
    /// Every template, in the jar's order.
    pub const ALL: [Template; 8] = [
        Template::Tiny,
        Template::Small,
        Template::Normal,
        Template::Large,
        Template::Giant,
        Template::Colossal,
        Template::Farming,
        Template::Soaryn,
    ];

    /// The template's path, as the jar names it (`compactmachines:tiny` → `tiny`).
    pub const fn id(self) -> &'static str {
        match self {
            Template::Tiny => "tiny",
            Template::Small => "small",
            Template::Normal => "normal",
            Template::Large => "large",
            Template::Giant => "giant",
            Template::Colossal => "colossal",
            Template::Farming => "farming",
            Template::Soaryn => "soaryn",
        }
    }

    /// The template a component id names, bare or namespaced.
    pub fn from_id(id: &str) -> Option<Template> {
        let short = id.strip_prefix("compactmachines:").unwrap_or(id);
        Template::ALL.into_iter().find(|it| it.id() == short)
    }

    /// The interior size (width, height, depth) from the jar's template file.
    pub const fn dims(self) -> [u32; 3] {
        match self {
            Template::Tiny => [3, 3, 3],
            Template::Small => [5, 5, 5],
            Template::Normal => [7, 7, 7],
            Template::Large => [9, 9, 9],
            Template::Giant => [11, 11, 11],
            Template::Colossal => [13, 13, 13],
            Template::Farming => [21, 11, 21],
            Template::Soaryn => [45, 45, 45],
        }
    }

    /// The template's machine colour (ARGB without alpha), from the same file.
    pub const fn color(self) -> [u8; 3] {
        match self {
            Template::Tiny => [0xC9, 0x5B, 0x13],
            Template::Small => [0xD4, 0xD2, 0xD2],
            Template::Normal => [0xFB, 0xF2, 0x36],
            Template::Large => [0x21, 0x1B, 0x2E],
            Template::Giant => [0x43, 0xD6, 0xCD],
            Template::Colossal => [0x42, 0x3F, 0x42],
            Template::Farming => [0x00, 0xFF, 0x00],
            Template::Soaryn => [0x7B, 0x2F, 0xBE],
        }
    }

    /// The colour as the jar writes it in `machine_color` (`#FBF236`).
    pub fn color_hex(self) -> String {
        let [r, g, b] = self.color();
        format!("#{r:02X}{g:02X}{b:02X}")
    }
}

// ---------------------------------------------------------------------------
// Codes and the region spiral
// ---------------------------------------------------------------------------

/// A deterministic xorshift64* generator, so room codes can be tested.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Seeds the generator; a zero seed is moved off zero (xorshift needs it).
    pub const fn new(seed: u64) -> Rng {
        Rng {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    /// The next value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A draw in `0..bound` (`bound` at most 2^32; the caller keeps it small).
    pub fn below(&mut self, bound: u32) -> u32 {
        (self.next_u64() % u64::from(bound)) as u32
    }
}

/// A fresh room code, `XXXX-XXXX-XXXX` (`RoomCodeGenerator.generateRoomId`).
pub fn generate_code(rng: &mut Rng) -> String {
    let letters: Vec<char> = ALPHABET.chars().collect();
    let mut body = String::with_capacity(12);
    for _ in 0..12 {
        body.push(letters[rng.below(letters.len() as u32) as usize]);
    }
    let body = body.as_str();
    format!("{}-{}-{}", &body[..4], &body[4..8], &body[8..])
}

/// Whether `code` is one of this mod's room codes.
pub fn is_room_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    bytes.len() == 14
        && bytes[4] == b'-'
        && bytes[9] == b'-'
        && bytes.iter().enumerate().all(|(index, byte)| {
            index == 4
                || index == 9
                || ALPHABET.as_bytes().contains(byte)
        })
}

/// The square spiral the jar uses to place rooms
/// (`MathUtil.getRegionPositionByIndex`): index 0 at the centre, then the
/// region below it, then right, right, up, up, left, left...
pub fn region_position(index: u32) -> [i32; 2] {
    let i = i64::from(index);
    let index = i + 1;
    let root = (index as f64).sqrt().ceil() as i64;
    let size = root + ((root % 2 + 1) % 2);
    let mut ring_index = 0;
    let mut perimeter = 1;
    if size > 1 {
        ring_index = i - (size - 2) * (size - 2);
        perimeter = size * size - (size - 2) * (size - 2);
    }
    let quarter = perimeter / 4;
    let ri = (ring_index + size / 2) % perimeter;
    let mut x = 0;
    let mut z = 0;
    if size > 1 {
        x = if ri < quarter {
            ri
        } else if ri < quarter * 2 {
            quarter
        } else if ri <= quarter * 3 {
            quarter * 3 - ri
        } else {
            0
        };
        z = if ri < quarter {
            0
        } else if ri < quarter * 2 {
            ri - quarter
        } else if ri <= quarter * 3 {
            quarter
        } else {
            perimeter - ri
        };
    }
    x -= size / 2;
    z -= size / 2;
    [x as i32, z as i32]
}

/// The block at the centre of a region (the block (8, y, 8) of chunk
/// `region * 64`, `MathUtil.getCenterWithY`).
pub fn site_center(region: [i32; 2]) -> [i32; 3] {
    [
        ROOM_BASE[0] + region[0] * REGION_BLOCKS + 8,
        ROOM_BASE[1],
        ROOM_BASE[2] + region[1] * REGION_BLOCKS + 8,
    ]
}

// ---------------------------------------------------------------------------
// Room geometry
// ---------------------------------------------------------------------------

/// A room's interior box: its lowest corner and interior size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomBox {
    pub min: [i32; 3],
    pub dims: [u32; 3],
}

impl RoomBox {
    /// A box with this lowest interior corner and interior size.
    pub const fn new(min: [i32; 3], dims: [u32; 3]) -> RoomBox {
        RoomBox { min, dims }
    }

    /// The shell's size: the interior inflated by one block on every side.
    pub const fn outer_size(&self) -> [u32; 3] {
        [self.dims[0] + 2, self.dims[1] + 2, self.dims[2] + 2]
    }

    /// Whether a block is inside the air volume (the shell is not).
    pub fn interior_contains(&self, pos: [i32; 3]) -> bool {
        (0..3).all(|axis| {
            pos[axis] >= self.min[axis]
                && pos[axis] < self.min[axis] + self.dims[axis] as i32
        })
    }

    /// The room's spawn: the interior's centre column, feet on the floor
    /// (`IRoomBoundaries.defaultSpawn`, one block lower — see FINDINGS §4).
    pub fn spawn(&self) -> [i32; 3] {
        [
            self.min[0] + (self.dims[0] as i32) / 2,
            self.min[1],
            self.min[2] + (self.dims[2] as i32) / 2,
        ]
    }

    /// Every block of the one-block shell: the inflated box minus the interior.
    pub fn shell_blocks(&self) -> Vec<[i32; 3]> {
        let mut blocks = Vec::with_capacity(
            (self.outer_size()[0] * self.outer_size()[1] * self.outer_size()[2]) as usize,
        );
        for x in self.min[0] - 1..self.min[0] + self.dims[0] as i32 + 1 {
            for y in self.min[1] - 1..self.min[1] + self.dims[1] as i32 + 1 {
                for z in self.min[2] - 1..self.min[2] + self.dims[2] as i32 + 1 {
                    let pos = [x, y, z];
                    if !self.interior_contains(pos) {
                        blocks.push(pos);
                    }
                }
            }
        }
        blocks
    }
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// A room: identity, template, site, owner and the point its players return to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub code: String,
    pub template: Template,
    /// The region index the room sits on (its site is `site_center`).
    pub site: [i32; 2],
    /// Whether the shell has been placed (it is built once, on first entry).
    pub built: bool,
    pub owner: u128,
    /// The dimension the machine (and so the return point) is in.
    pub dim: String,
    /// Where a player leaving this room is put back.
    pub entry: [i32; 3],
}

/// What a machine block at a position is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A placed `new_machine`: a room is created from this template on first use.
    Pending { template: Template, owner: u128 },
    /// A bound `machine`: the room it opens.
    Bound { code: String },
}

/// A machine block: its dimension and position.
pub type MachineKey = (String, [i32; 3]);

/// One entry of a player's room history: the room and where they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub code: String,
    pub dim: String,
    pub pos: [i32; 3],
}

/// What entering a room does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enter {
    Entered { spawn: [i32; 3] },
    /// The player is already `max_depth` rooms deep.
    TooDeep,
    NoSuchRoom,
}

/// The campaign's machine/room state, persisted beside the world.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Store {
    next_region: u32,
    rooms: BTreeMap<String, Room>,
    machines: BTreeMap<MachineKey, Binding>,
    history: BTreeMap<u128, Vec<History>>,
}

impl Store {
    /// An empty store.
    pub fn new() -> Store {
        Store::default()
    }

    /// The region index the next room takes (diagnostics and tests).
    #[cfg(test)]
    pub fn next_region(&self) -> u32 {
        self.next_region
    }

    /// How many rooms are registered.
    pub fn rooms_len(&self) -> usize {
        self.rooms.len()
    }

    /// The rooms, by code.
    pub fn rooms(&self) -> impl Iterator<Item = &Room> {
        self.rooms.values()
    }

    /// One room.
    pub fn room(&self, code: &str) -> Option<&Room> {
        self.rooms.get(code)
    }

    /// A room's geometry.
    pub fn room_box(&self, code: &str) -> Option<RoomBox> {
        let room = self.rooms.get(code)?;
        let center = site_center(room.site);
        let dims = room.template.dims();
        Some(RoomBox::new(
            [
                center[0] - dims[0] as i32 / 2,
                center[1],
                center[2] - dims[2] as i32 / 2,
            ],
            dims,
        ))
    }

    /// Creates a room on the next free site and answers it.
    pub fn create_room(
        &mut self,
        template: Template,
        owner: u128,
        entry: [i32; 3],
        dim: &str,
        rng: &mut Rng,
    ) -> &Room {
        loop {
            let site = region_position(self.next_region);
            self.next_region += 1;
            if self.rooms.values().any(|room| room.site == site) {
                continue;
            }
            let mut code = generate_code(rng);
            while self.rooms.contains_key(&code) {
                code = generate_code(rng);
            }
            let room = Room {
                code: code.clone(),
                template,
                site,
                built: false,
                owner,
                dim: dim.to_owned(),
                entry,
            };
            self.rooms.insert(code.clone(), room);
            return self.rooms.get(&code).expect("the room was just inserted");
        }
    }

    /// Records that a room's shell exists.
    pub fn set_built(&mut self, code: &str, built: bool) {
        if let Some(room) = self.rooms.get_mut(code) {
            room.built = built;
        }
    }

    /// Binds a machine position to a room (or to a pending template).
    pub fn bind_machine(&mut self, key: MachineKey, binding: Binding) {
        self.machines.insert(key, binding);
    }

    /// What a machine position is bound to.
    pub fn binding(&self, key: &MachineKey) -> Option<&Binding> {
        self.machines.get(key)
    }

    /// Forgets a machine position.
    pub fn unbind_machine(&mut self, key: &MachineKey) {
        self.machines.remove(key);
    }

    /// The room whose interior holds `pos` in `dim`.
    pub fn room_at(&self, pos: [i32; 3], dim: &str) -> Option<&Room> {
        self.rooms.values().find(|room| {
            room.dim == dim
                && self
                    .room_box(&room.code)
                    .is_some_and(|room_box| room_box.interior_contains(pos))
        })
    }

    /// A player's room history, oldest first (diagnostics and tests).
    #[cfg(test)]
    pub fn history(&self, player: u128) -> &[History] {
        self.history.get(&player).map_or(&[], Vec::as_slice)
    }

    /// Queues an entry: refuses a missing room and a history that is too deep.
    pub fn enter(
        &mut self,
        player: u128,
        code: &str,
        entry: [i32; 3],
        dim: &str,
        max_depth: usize,
    ) -> Enter {
        if !self.rooms.contains_key(code) {
            return Enter::NoSuchRoom;
        }
        let Some(room_box) = self.room_box(code) else {
            return Enter::NoSuchRoom;
        };
        let spawn = room_box.spawn();
        let depth = self.history.get(&player).map_or(0, Vec::len);
        if depth >= max_depth {
            return Enter::TooDeep;
        }
        self.history.entry(player).or_default().push(History {
            code: code.to_owned(),
            dim: dim.to_owned(),
            pos: entry,
        });
        Enter::Entered { spawn }
    }

    /// Pops a player's last entry, answering where they came from.
    pub fn leave(&mut self, player: u128) -> Option<History> {
        let entries = self.history.get_mut(&player)?;
        let last = entries.pop();
        if entries.is_empty() {
            self.history.remove(&player);
        }
        last
    }

    /// The store's world file.
    pub fn encode(&self) -> String {
        let mut text = String::from("# compactmachines v1\n");
        text.push_str(&format!("next {}\n", self.next_region));
        for room in self.rooms.values() {
            text.push_str(&format!(
                "room {} {} {} {} {} {} {} {} {} {}\n",
                room.code,
                room.template.id(),
                u8::from(room.built),
                room.owner,
                room.site[0],
                room.site[1],
                room.dim,
                room.entry[0],
                room.entry[1],
                room.entry[2],
            ));
        }
        for (key, binding) in &self.machines {
            match binding {
                Binding::Pending { template, owner } => text.push_str(&format!(
                    "machine {} {} {} {} pending {} {}\n",
                    key.0, key.1[0], key.1[1], key.1[2], template.id(), owner
                )),
                Binding::Bound { code } => text.push_str(&format!(
                    "machine {} {} {} {} bound {}\n",
                    key.0, key.1[0], key.1[1], key.1[2], code
                )),
            }
        }
        for (player, entries) in &self.history {
            for (index, entry) in entries.iter().enumerate() {
                text.push_str(&format!(
                    "hist {player} {index} {} {} {} {} {}\n",
                    entry.code, entry.dim, entry.pos[0], entry.pos[1], entry.pos[2]
                ));
            }
        }
        text
    }

    /// Reads the world file; what it cannot read is reported, never fatal.
    pub fn decode(text: &str) -> (Store, Vec<String>) {
        let mut store = Store::new();
        let mut warnings = Vec::new();
        let mut histories: BTreeMap<u128, Vec<(usize, History)>> = BTreeMap::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            let warn = |what: &str| format!("line {}: {what}: {line}", number + 1);
            match fields.first().copied() {
                Some("next") if fields.len() == 2 => match fields[1].parse::<u32>() {
                    Ok(next) => store.next_region = next,
                    Err(_) => warnings.push(warn("next is not a number")),
                },
                Some("room") if fields.len() == 11 => {
                    let built = match fields[3] {
                        "0" => false,
                        "1" => true,
                        _ => {
                            warnings.push(warn("built is not 0 or 1"));
                            continue;
                        }
                    };
                    let (Some(template), Ok(owner), Ok(site_x), Ok(site_z), Ok(entry_x), Ok(
                        entry_y,
                    ), Ok(entry_z)) = (
                        Template::from_id(fields[2]),
                        fields[4].parse::<u128>(),
                        fields[5].parse::<i32>(),
                        fields[6].parse::<i32>(),
                        fields[8].parse::<i32>(),
                        fields[9].parse::<i32>(),
                        fields[10].parse::<i32>(),
                    ) else {
                        warnings.push(warn("room is malformed"));
                        continue;
                    };
                    if !is_room_code(fields[1]) {
                        warnings.push(warn("room code is not a room code"));
                        continue;
                    }
                    store.rooms.entry(fields[1].to_owned()).or_insert(Room {
                        code: fields[1].to_owned(),
                        template,
                        site: [site_x, site_z],
                        built,
                        owner,
                        dim: fields[7].to_owned(),
                        entry: [entry_x, entry_y, entry_z],
                    });
                }
                Some("machine") if fields.len() == 7 || fields.len() == 8 => {
                    let (Ok(x), Ok(y), Ok(z)) = (
                        fields[2].parse::<i32>(),
                        fields[3].parse::<i32>(),
                        fields[4].parse::<i32>(),
                    ) else {
                        warnings.push(warn("machine position is malformed"));
                        continue;
                    };
                    let key = (fields[1].to_owned(), [x, y, z]);
                    match (fields[5], fields.len()) {
                        ("pending", 8) => match (
                            Template::from_id(fields[6]),
                            fields[7].parse::<u128>(),
                        ) {
                            (Some(template), Ok(owner)) => {
                                store.machines.insert(
                                    key,
                                    Binding::Pending { template, owner },
                                );
                            }
                            _ => {
                                warnings.push(warn("pending machine is malformed"));
                                continue;
                            }
                        },
                        ("bound", 7) => {
                            if is_room_code(fields[6]) {
                                store.machines.insert(
                                    key,
                                    Binding::Bound {
                                        code: fields[6].to_owned(),
                                    },
                                );
                            } else {
                                warnings.push(warn("bound machine names no room code"));
                                continue;
                            }
                        }
                        _ => {
                            warnings.push(warn("machine binding is malformed"));
                            continue;
                        }
                    };
                }
                Some("hist") if fields.len() == 8 => {
                    let (Ok(player), Ok(index), Ok(x), Ok(y), Ok(z)) = (
                        fields[1].parse::<u128>(),
                        fields[2].parse::<usize>(),
                        fields[5].parse::<i32>(),
                        fields[6].parse::<i32>(),
                        fields[7].parse::<i32>(),
                    ) else {
                        warnings.push(warn("history is malformed"));
                        continue;
                    };
                    if !is_room_code(fields[3]) {
                        warnings.push(warn("history names no room code"));
                        continue;
                    }
                    histories.entry(player).or_default().push((
                        index,
                        History {
                            code: fields[3].to_owned(),
                            dim: fields[4].to_owned(),
                            pos: [x, y, z],
                        },
                    ));
                }
                Some("next") | Some("room") | Some("machine") | Some("hist") => {
                    warnings.push(warn("wrong number of fields"));
                }
                Some(other) => warnings.push(format!("line {}: unknown {other}", number + 1)),
                None => {}
            }
        }
        for (player, mut entries) in histories {
            entries.sort_by_key(|(index, _)| *index);
            store
                .history
                .insert(player, entries.into_iter().map(|(_, entry)| entry).collect());
        }
        (store, warnings)
    }
}

// ---------------------------------------------------------------------------
// Commands and teleports
// ---------------------------------------------------------------------------

/// A `/compactmachines` line, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Rooms,
    Info,
    GiveNew { template: Template },
    GiveExisting { code: String },
    Teleport { code: String },
    Unknown,
}

/// Parses the arguments after `compactmachines`.
pub fn parse(args: &[&str]) -> Command {
    match args {
        [] => Command::Help,
        ["rooms"] => Command::Rooms,
        ["info"] => Command::Info,
        ["give", "new", template] => match Template::from_id(template) {
            Some(template) => Command::GiveNew { template },
            None => Command::Unknown,
        },
        ["give", "existing", code] | ["teleport", code] if is_room_code(code) => {
            if args[1] == "existing" {
                Command::GiveExisting {
                    code: (*code).to_owned(),
                }
            } else {
                Command::Teleport {
                    code: (*code).to_owned(),
                }
            }
        }
        _ => Command::Unknown,
    }
}

/// The deferred console teleport the pin needs: there is no player-position
/// write API, and `native_run_command` is refused inside a player hook.
pub fn teleport_line(dim: &str, pos: [i32; 3], player: u128) -> String {
    format!(
        "execute in {dim} run tp {} {}.5 {} {}.5",
        hyphenate_uuid(player),
        pos[0],
        pos[1],
        pos[2]
    )
}

/// A player UUID in vanilla's hyphenated form.
pub fn hyphenate_uuid(uuid: u128) -> String {
    let digits = format!("{uuid:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &digits[0..8],
        &digits[8..12],
        &digits[12..16],
        &digits[16..20],
        &digits[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- templates: the jar's own numbers -------------------------------

    #[test]
    fn templates_match_the_jars_room_templates() {
        let want = [
            (Template::Tiny, "tiny", [3, 3, 3], [0xC9, 0x5B, 0x13]),
            (Template::Small, "small", [5, 5, 5], [0xD4, 0xD2, 0xD2]),
            (Template::Normal, "normal", [7, 7, 7], [0xFB, 0xF2, 0x36]),
            (Template::Large, "large", [9, 9, 9], [0x21, 0x1B, 0x2E]),
            (Template::Giant, "giant", [11, 11, 11], [0x43, 0xD6, 0xCD]),
            (Template::Colossal, "colossal", [13, 13, 13], [0x42, 0x3F, 0x42]),
            (Template::Farming, "farming", [21, 11, 21], [0x00, 0xFF, 0x00]),
            (Template::Soaryn, "soaryn", [45, 45, 45], [0x7B, 0x2F, 0xBE]),
        ];
        assert_eq!(Template::ALL.len(), want.len());
        let hex = [
            "#C95B13", "#D4D2D2", "#FBF236", "#211B2E", "#43D6CD", "#423F42", "#00FF00",
            "#7B2FBE",
        ];
        for ((template, id, dims, color), hex) in want.into_iter().zip(hex) {
            assert_eq!(template.id(), id);
            assert_eq!(template.dims(), dims, "{id}");
            assert_eq!(template.color(), color, "{id}");
            assert_eq!(template.color_hex(), hex, "{id}");
        }
    }

    #[test]
    fn template_ids_round_trip() {
        for template in Template::ALL {
            assert_eq!(Template::from_id(template.id()), Some(template));
            assert_eq!(
                Template::from_id(&format!("compactmachines:{}", template.id())),
                Some(template)
            );
        }
        assert_eq!(Template::from_id("empty"), None);
        assert_eq!(Template::from_id(""), None);
        assert_eq!(Template::from_id("compactmachines:"), None);
    }

    // ---- room codes ------------------------------------------------------

    #[test]
    fn code_has_the_upstream_shape() {
        let mut rng = Rng::new(0x5eed);
        for _ in 0..100 {
            let code = generate_code(&mut rng);
            assert_eq!(code.len(), 14, "{code}");
            let bytes = code.as_bytes();
            assert_eq!(bytes[4], b'-', "{code}");
            assert_eq!(bytes[9], b'-', "{code}");
            for (i, byte) in bytes.iter().enumerate() {
                if i == 4 || i == 9 {
                    continue;
                }
                assert!(
                    ALPHABET.contains(*byte as char),
                    "{code}: byte {i} = {}",
                    *byte as char
                );
            }
        }
    }

    #[test]
    fn the_alphabet_has_no_ambiguous_letters() {
        // The jar's own 31 characters: 10 digits and 21 letters, no C, E, I,
        // O or U (they are easy to confuse with digits or each other).
        assert_eq!(ALPHABET.len(), 31);
        for letter in ['C', 'E', 'I', 'O', 'U'] {
            assert!(
                !ALPHABET.contains(letter),
                "{letter} must not be in the alphabet"
            );
        }
    }

    #[test]
    fn a_thousand_codes_are_all_different() {
        let mut rng = Rng::new(1);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(generate_code(&mut rng)));
        }
    }

    #[test]
    fn the_rng_is_deterministic_for_a_seed() {
        let a: Vec<u64> = {
            let mut rng = Rng::new(7);
            (0..8).map(|_| rng.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut rng = Rng::new(7);
            (0..8).map(|_| rng.next_u64()).collect()
        };
        assert_eq!(a, b);
        assert!(a.windows(2).any(|w| w[0] != w[1]));
    }

    // ---- region spiral: upstream's room layout ---------------------------

    #[test]
    fn region_positions_are_the_upstream_spiral() {
        // MathUtil.getRegionPositionByIndex: index 0 at the centre, then the
        // region below it, then right, right, up, up, left, left...
        let want = [
            [0, 0],
            [0, -1],
            [1, -1],
            [1, 0],
            [1, 1],
            [0, 1],
            [-1, 1],
            [-1, 0],
            [-1, -1],
            [0, -2],
        ];
        for (index, expected) in want.into_iter().enumerate() {
            assert_eq!(region_position(index as u32), expected, "index {index}");
        }
    }

    #[test]
    fn region_positions_never_repeat_over_the_first_thousand() {
        let mut seen = std::collections::BTreeSet::new();
        for index in 0..1000u32 {
            assert!(seen.insert(region_position(index)), "index {index}");
        }
    }

    #[test]
    fn sites_are_1024_apart_and_centred_in_their_chunk() {
        let a = site_center([0, 0]);
        assert_eq!(a, [ROOM_BASE[0] + 8, ROOM_BASE[1], ROOM_BASE[2] + 8]);
        let b = site_center([1, 0]);
        assert_eq!(b[0] - a[0], 1024);
        assert_eq!(b[2], a[2]);
        let c = site_center([0, -1]);
        assert_eq!(c[2] - a[2], -1024);
    }

    // ---- room geometry ---------------------------------------------------

    #[test]
    fn a_7_room_has_386_shell_blocks_and_343_interior_blocks() {
        let room = RoomBox::new([0, 0, 0], [7, 7, 7]);
        assert_eq!(room.shell_blocks().len(), 9 * 9 * 9 - 7 * 7 * 7);
        assert_eq!(room.shell_blocks().len(), 386);
        assert_eq!(room.outer_size(), [9, 9, 9]);
    }

    #[test]
    fn a_3_room_has_5_cubed_minus_27_shell_blocks() {
        let room = RoomBox::new([10, 20, 30], [3, 3, 3]);
        assert_eq!(room.shell_blocks().len(), 125 - 27);
    }

    #[test]
    fn the_shell_is_exactly_one_layer_thick() {
        let room = RoomBox::new([-5, 40, 7], [5, 5, 5]);
        let shell: std::collections::BTreeSet<[i32; 3]> =
            room.shell_blocks().into_iter().collect();
        assert_eq!(shell.len(), 7 * 7 * 7 - 5 * 5 * 5);
        for block in &shell {
            let outer_min = [room.min[0] - 1, room.min[1] - 1, room.min[2] - 1];
            let outer_max = [
                room.min[0] + room.dims[0] as i32,
                room.min[1] + room.dims[1] as i32,
                room.min[2] + room.dims[2] as i32,
            ];
            let on_boundary = (0..3).any(|axis| {
                block[axis] == outer_min[axis] || block[axis] == outer_max[axis]
            });
            assert!(on_boundary, "{block:?} is inside the wall, not on it");
        }
        for x in room.min[0]..room.min[0] + room.dims[0] as i32 {
            for y in room.min[1]..room.min[1] + room.dims[1] as i32 {
                for z in room.min[2]..room.min[2] + room.dims[2] as i32 {
                    for offset in NEIGHBOURS {
                        let neighbour = [x + offset[0], y + offset[1], z + offset[2]];
                        if !room.interior_contains(neighbour) {
                            assert!(
                                shell.contains(&neighbour),
                                "hole at {neighbour:?} beside {x},{y},{z}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn spawn_is_the_interior_centre_on_the_floor() {
        let room = RoomBox::new([100, 200, 300], [7, 7, 7]);
        assert_eq!(room.spawn(), [103, 200, 303]);
        let odd = RoomBox::new([0, 0, 0], [3, 3, 3]);
        assert_eq!(odd.spawn(), [1, 0, 1]);
        let flat = RoomBox::new([0, 0, 0], [21, 11, 21]);
        assert_eq!(flat.spawn(), [10, 0, 10]);
    }

    #[test]
    fn the_interior_is_air_space_and_the_shell_is_not() {
        let room = RoomBox::new([0, 0, 0], [3, 3, 3]);
        assert!(room.interior_contains([0, 0, 0]));
        assert!(room.interior_contains([2, 2, 2]));
        assert!(!room.interior_contains([-1, 0, 0]));
        assert!(!room.interior_contains([3, 0, 0]));
        assert!(!room.interior_contains([0, 3, 0]));
        assert!(!room.interior_contains([0, -1, 0]));
    }

    // ---- the store -------------------------------------------------------

    fn store_with_room(seed: u64, template: Template) -> (Store, String) {
        let mut store = Store::new();
        let mut rng = Rng::new(seed);
        let code = store
            .create_room(
                template,
                42,
                [100, 64, 100],
                "minecraft:overworld",
                &mut rng,
            )
            .code
            .clone();
        (store, code)
    }

    #[test]
    fn a_new_room_takes_the_next_site_and_the_template_size() {
        let mut store = Store::new();
        let mut rng = Rng::new(11);
        let normal = store
            .create_room(
                Template::Normal,
                42,
                [100, 64, 100],
                "minecraft:overworld",
                &mut rng,
            )
            .code
            .clone();
        let tiny = store
            .create_room(
                Template::Tiny,
                42,
                [100, 64, 100],
                "minecraft:overworld",
                &mut rng,
            )
            .code
            .clone();
        assert_ne!(normal, tiny);
        let a = store.room(&normal).expect("normal room");
        let b = store.room(&tiny).expect("tiny room");
        assert_eq!(store.room_box(&normal).unwrap().dims, [7, 7, 7]);
        assert_eq!(store.room_box(&tiny).unwrap().dims, [3, 3, 3]);
        assert_ne!(a.site, b.site, "two rooms must never share a site");
        assert_eq!(store.next_region(), 2);
    }

    #[test]
    fn a_machine_binds_to_exactly_one_room() {
        let (mut store, code) = store_with_room(12, Template::Normal);
        let key = ("minecraft:overworld".to_owned(), [10, 70, 10]);
        store.bind_machine(
            key.clone(),
            Binding::Bound { code: code.clone() },
        );
        assert_eq!(
            store.binding(&key),
            Some(&Binding::Bound { code: code.clone() })
        );
        store.unbind_machine(&key);
        assert_eq!(store.binding(&key), None);
    }

    #[test]
    fn entering_an_unknown_room_is_refused() {
        let mut store = Store::new();
        assert_eq!(
            store.enter(
                7,
                "AAAA-BBBB-DDDD",
                [0, 64, 0],
                "minecraft:overworld",
                MAX_ENTRY_DEPTH
            ),
            Enter::NoSuchRoom
        );
        assert!(store.history(7).is_empty());
    }

    #[test]
    fn entering_an_existing_room_lands_on_its_spawn() {
        let (mut store, code) = store_with_room(13, Template::Normal);
        let wanted = store.room_box(&code).unwrap().spawn();
        assert_eq!(
            store.enter(
                7,
                &code,
                [100, 64, 100],
                "minecraft:overworld",
                MAX_ENTRY_DEPTH
            ),
            Enter::Entered { spawn: wanted }
        );
        assert_eq!(store.history(7).len(), 1);
    }

    #[test]
    fn five_nested_entries_are_allowed_and_the_sixth_is_not() {
        let (mut store, code) = store_with_room(14, Template::Tiny);
        for _ in 0..MAX_ENTRY_DEPTH {
            assert!(matches!(
                store.enter(7, &code, [0, 64, 0], "minecraft:overworld", MAX_ENTRY_DEPTH),
                Enter::Entered { .. }
            ));
        }
        assert_eq!(
            store.enter(7, &code, [0, 64, 0], "minecraft:overworld", MAX_ENTRY_DEPTH),
            Enter::TooDeep
        );
        assert_eq!(store.history(7).len(), MAX_ENTRY_DEPTH);
    }

    #[test]
    fn leaving_pops_the_last_entry_and_answers_where_to_go() {
        let (mut store, code) = store_with_room(15, Template::Normal);
        store.enter(
            7,
            &code,
            [0, 64, 0],
            "minecraft:overworld",
            MAX_ENTRY_DEPTH,
        );
        let left = store.leave(7).expect("a history entry");
        assert_eq!(left.code, code);
        assert_eq!(left.pos, [0, 64, 0]);
        assert_eq!(left.dim, "minecraft:overworld");
        assert_eq!(store.leave(7), None);
    }

    #[test]
    fn a_player_standing_in_a_room_is_found_by_its_interior_only() {
        let (store, code) = store_with_room(16, Template::Normal);
        let spawn = store.room_box(&code).unwrap().spawn();
        assert_eq!(
            store
                .room_at(spawn, "minecraft:overworld")
                .map(|room| room.code.clone()),
            Some(code.clone())
        );
        assert_eq!(
            store
                .room_at([spawn[0] + 40, spawn[1], spawn[2]], "minecraft:overworld")
                .map(|room| room.code.clone()),
            None
        );
        assert_eq!(
            store
                .room_at(spawn, "minecraft:the_nether")
                .map(|room| room.code.clone()),
            None
        );
    }

    #[test]
    fn a_kept_room_keeps_its_site_when_a_later_one_is_created() {
        let (mut store, first) = store_with_room(17, Template::Normal);
        let site = store.room(&first).unwrap().site;
        let mut rng = Rng::new(99);
        for _ in 0..10 {
            store.create_room(
                Template::Tiny,
                1,
                [0, 64, 0],
                "minecraft:overworld",
                &mut rng,
            );
        }
        assert_eq!(store.room(&first).unwrap().site, site);
    }

    // ---- the world file --------------------------------------------------

    #[test]
    fn the_store_round_trips_through_its_file() {
        let mut store = Store::new();
        let mut rng = Rng::new(21);
        let normal = store
            .create_room(
                Template::Normal,
                42,
                [100, 64, 100],
                "minecraft:overworld",
                &mut rng,
            )
            .code
            .clone();
        let tiny = store
            .create_room(
                Template::Tiny,
                43,
                [200, 64, 200],
                "minecraft:overworld",
                &mut rng,
            )
            .code
            .clone();
        store.bind_machine(
            ("minecraft:overworld".to_owned(), [100, 64, 100]),
            Binding::Bound {
                code: normal.clone(),
            },
        );
        store.bind_machine(
            ("minecraft:overworld".to_owned(), [200, 64, 200]),
            Binding::Pending {
                template: Template::Farming,
                owner: 43,
            },
        );
        store.enter(
            7,
            &normal,
            [100, 64, 100],
            "minecraft:overworld",
            MAX_ENTRY_DEPTH,
        );
        store.enter(8, &tiny, [200, 64, 200], "minecraft:the_nether", MAX_ENTRY_DEPTH);
        store.set_built(&tiny, true);

        let text = store.encode();
        let (back, warnings) = Store::decode(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back.encode(), text);
        assert_eq!(back.next_region(), store.next_region());
        assert_eq!(back.room(&normal), store.room(&normal));
        assert_eq!(back.room(&tiny), store.room(&tiny));
        assert_eq!(
            back.binding(&("minecraft:overworld".to_owned(), [100, 64, 100])),
            store.binding(&("minecraft:overworld".to_owned(), [100, 64, 100]))
        );
        assert_eq!(back.history(7), store.history(7));
        assert_eq!(back.history(8), store.history(8));
    }

    #[test]
    fn a_broken_line_is_reported_and_the_rest_still_loads() {
        let text = "\
# compactmachines v1
next 3
room QQQQ-WWWW-FFFF normal 0 42 30000 30000 minecraft:overworld 100 64 100
room nope
machine minecraft:overworld 1 2 3 bound QQQQ-WWWW-FFFF
machine minecraft:overworld 1 2 3 sideways X
hist 7 0 QQQQ-WWWW-FFFF minecraft:overworld 100 64 100
nonsense line
";
        let (store, warnings) = Store::decode(text);
        assert_eq!(store.rooms_len(), 1);
        assert!(store.room("QQQQ-WWWW-FFFF").is_some());
        assert_eq!(store.next_region(), 3);
        assert_eq!(store.history(7).len(), 1);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }

    #[test]
    fn an_empty_file_is_an_empty_store() {
        let (store, warnings) = Store::decode("");
        assert_eq!(store.rooms_len(), 0);
        assert!(warnings.is_empty());
        assert_eq!(store.next_region(), 0);
    }

    #[test]
    fn a_room_line_needs_the_template_the_jar_has() {
        let text = "room AAAA-BBBB-DDDD sideways 0 1 30000 30000 minecraft:overworld 0 64 0\n";
        let (store, warnings) = Store::decode(text);
        assert_eq!(store.rooms_len(), 0);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn a_file_that_forgot_its_next_still_skips_a_used_site() {
        // `next` behind a room's site: the room keeps its site and the next
        // room lands elsewhere (`create_room` skips an occupied region).
        let text = "room AAAA-BBBB-DDDD tiny 0 1 29992 29992 minecraft:overworld 0 64 0\n";
        let (mut store, warnings) = Store::decode(text);
        assert!(warnings.is_empty(), "{warnings:?}");
        let kept = store.room("AAAA-BBBB-DDDD").unwrap().site;
        let mut rng = Rng::new(5);
        let code = store
            .create_room(
                Template::Tiny,
                1,
                [0, 64, 0],
                "minecraft:overworld",
                &mut rng,
            )
            .code
            .clone();
        assert_ne!(store.room(&code).unwrap().site, kept);
    }

    // ---- commands and teleports ------------------------------------------

    #[test]
    fn the_command_lines_parse() {
        assert_eq!(parse(&[]), Command::Help);
        assert_eq!(parse(&["rooms"]), Command::Rooms);
        assert_eq!(parse(&["info"]), Command::Info);
        assert_eq!(
            parse(&["give", "new", "compactmachines:tiny"]),
            Command::GiveNew {
                template: Template::Tiny
            }
        );
        assert_eq!(
            parse(&["give", "existing", "AAAA-BBBB-DDDD"]),
            Command::GiveExisting {
                code: "AAAA-BBBB-DDDD".to_owned()
            }
        );
        assert_eq!(
            parse(&["teleport", "AAAA-BBBB-DDDD"]),
            Command::Teleport {
                code: "AAAA-BBBB-DDDD".to_owned()
            }
        );
        assert_eq!(parse(&["give", "new", "nonsense"]), Command::Unknown);
        assert_eq!(parse(&["give"]), Command::Unknown);
        assert_eq!(parse(&["teleport"]), Command::Unknown);
        assert_eq!(parse(&["wat"]), Command::Unknown);
        assert_eq!(parse(&["teleport", "aaaa-bbbb-cccc"]), Command::Unknown);
    }

    #[test]
    fn a_teleport_line_is_the_deferred_console_command() {
        assert_eq!(
            teleport_line("minecraft:overworld", [103, 200, 303], 1),
            "execute in minecraft:overworld run tp \
             00000000-0000-0000-0000-000000000001 103.5 200 303.5"
        );
    }

    #[test]
    fn uuid_hyphenation_is_vanillas() {
        assert_eq!(hyphenate_uuid(1), "00000000-0000-0000-0000-000000000001");
        assert_eq!(
            hyphenate_uuid(0x1234_5678_9abc_def0_1122_3344_5566_7788),
            "12345678-9abc-def0-1122-334455667788"
        );
    }

    #[test]
    fn a_room_code_is_recognised() {
        assert!(is_room_code("AAAA-BBBB-DDDD"));
        assert!(is_room_code("0123-4567-89AB"));
        assert!(!is_room_code("aaaa-bbbb-cccc"), "lower case is not a code");
        assert!(!is_room_code("AAAA-BBBB-CCC"));
        assert!(!is_room_code("AAAA-BBBB-DDDD "));
        assert!(!is_room_code("AAAA-BBBB-DDDE"), "E is not in the alphabet");
    }

    #[test]
    fn the_components_a_machine_can_carry_are_the_jars() {
        assert_eq!(ROOM_TEMPLATE_COMPONENT, "compactmachines:room_template");
        assert_eq!(ROOM_CODE_COMPONENT, "compactmachines:room_code");
        assert_eq!(MACHINE_COLOR_COMPONENT, "compactmachines:machine_color");
    }

    const NEIGHBOURS: [[i32; 3]; 6] = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ];
}
