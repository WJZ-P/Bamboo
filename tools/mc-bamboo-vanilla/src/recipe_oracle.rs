//! 用官方的 `RecipeManager` 检查 `mc-bamboo-recipes`。
//!
//! `RecipeOracle.java` 在 JVM 里对大量合成网格、烧炼输入、切石机输入、锻造台输入和酿造输入查配方，
//! 一题一行写到 stdout。这里边读边用同样的输入重放，比较三样东西：
//!
//! - 这个类型里全部匹配的配方，按注册表顺序；
//! - `getRecipeFor` 找到的第一条；
//! - 第一条配方产物的物品和数量。
//!
//! 产物只比物品和数量，组件在 Bamboo 里还是不透明字节。纹饰的产物按官方取基础物品、数量 1；
//! 转化配方按官方沿用的输入物品算物品，数量是配方数量加上材料数。
//!
//! 官方那边跳过的题算失败：跳题说明有输入让官方代码抛异常，这种题必须弄明白。

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_data::{Recipe, RecipeKind, RecipeType, Stack};
use mc_bamboo_recipes::{
    BrewingInput, CraftingInput, ItemStack, RecipeInput, RecipeManager, SmithingInput,
};
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str = "usage: recipe-oracle <java> [--cases N] [--seed S] [--show N]";
/// `--show` 的默认值。
const DEFAULT_SHOW: usize = 3;

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: Option<usize>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--cases" | "--seed" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = Some(value.parse().map_err(|_| USAGE.to_owned())?);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

/// 官方那一行 JSON。缺的字段就是空值：Gson 不写 `null` 的成员。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Line {
    kind: String,
    #[serde(default)]
    width: usize,
    #[serde(default)]
    height: usize,
    #[serde(default)]
    slots: Vec<Option<StackJson>>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    input: Option<StackJson>,
    #[serde(default)]
    template: Option<StackJson>,
    #[serde(default)]
    base: Option<StackJson>,
    #[serde(default)]
    addition: Option<StackJson>,
    #[serde(default)]
    reagent: Option<StackJson>,
    #[serde(default)]
    matches: Vec<String>,
    #[serde(default)]
    first: Option<String>,
    #[serde(default)]
    result: Option<StackJson>,
    #[serde(default)]
    assemble_error: Option<String>,
    #[serde(default)]
    cases: usize,
    #[serde(default)]
    skipped: usize,
    #[serde(default)]
    skip_reasons: BTreeMap<String, usize>,
}

/// 官方的一个物品栈，组件只留药水 ID。
#[derive(Debug, Deserialize)]
struct StackJson {
    item: i32,
    count: i32,
    #[serde(default)]
    potion: Option<i32>,
}

impl StackJson {
    fn stack(&self) -> ItemStack {
        ItemStack::new(self.item, self.count)
    }
}

/// 题目的输入，四种工作台各一种。
enum CaseInput {
    Crafting(CraftingInput),
    Stack(ItemStack),
    Smithing(SmithingInput),
    Brewing(BrewingInput),
}

impl RecipeInput for CaseInput {
    fn is_empty(&self) -> bool {
        match self {
            Self::Crafting(input) => input.is_empty(),
            Self::Stack(stack) => stack.is_empty(),
            Self::Smithing(input) => input.is_empty(),
            Self::Brewing(input) => input.is_empty(),
        }
    }

    fn matches(&self, recipe: &Recipe) -> bool {
        match self {
            Self::Crafting(input) => input.matches(recipe),
            Self::Stack(stack) => stack.matches(recipe),
            Self::Smithing(input) => input.matches(recipe),
            Self::Brewing(input) => input.matches(recipe),
        }
    }
}

/// 一次运行的计数和不一致。
#[derive(Debug, Default)]
struct Report {
    cases: usize,
    crafting: usize,
    cooking: usize,
    stonecutting: usize,
    smithing: usize,
    brewing: usize,
    mismatches: Vec<String>,
    show: usize,
    summary: Option<(usize, usize, BTreeMap<String, usize>)>,
}

/// 启动官方进程，一边读题一边比较。`command` 已经是能运行 `RecipeOracle.java` 的命令。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;

    let mut report = Report {
        show: options.show.unwrap_or(DEFAULT_SHOW),
        ..Report::default()
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        // 这边不再读了，JVM 会一直等着管道。
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("RecipeOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            // Minecraft 的日志也写到 stdout，题目行都是 JSON 对象。
            if !line.starts_with('{') {
                continue;
            }
            let case: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad recipe oracle line: {error}"))?;
            if case.kind == "summary" {
                self.summary = Some((case.cases, case.skipped, case.skip_reasons));
                continue;
            }
            self.cases += 1;
            match case.kind.as_str() {
                "crafting" => self.crafting += 1,
                "cooking" => self.cooking += 1,
                "stonecutting" => self.stonecutting += 1,
                "smithing" => self.smithing += 1,
                "brewing" => self.brewing += 1,
                other => return Err(format!("unknown recipe case kind {other}")),
            }
            self.check(&case)?;
        }
        Ok(())
    }

    fn check(&mut self, case: &Line) -> Result<(), String> {
        let input = build_input(case)?;
        let kind = recipe_type(case)?;
        let manager = RecipeManager::global();
        let ours: Vec<&str> = manager
            .matching(kind, &input)
            .map(|recipe| recipe.id)
            .collect();
        let expected: Vec<&str> = case.matches.iter().map(String::as_str).collect();
        if ours != expected {
            self.mismatch(format!(
                "{}: matches official {:?}, bamboo {:?}",
                describe(case),
                expected,
                ours
            ));
            return Ok(());
        }

        let first = manager.get_recipe_for(kind, &input, None);
        if first.map(|recipe| recipe.id) != case.first.as_deref() {
            self.mismatch(format!(
                "{}: first official {:?}, bamboo {:?}",
                describe(case),
                case.first.as_deref(),
                first.map(|recipe| recipe.id)
            ));
            return Ok(());
        }

        let Some(first) = first else {
            return Ok(());
        };
        if let Some(error) = &case.assemble_error {
            self.mismatch(format!(
                "{}: official assemble of {} threw {error}",
                describe(case),
                first.id
            ));
            return Ok(());
        }
        let ours = expected_result(first, &input);
        let official = case.result.as_ref().map(|stack| (stack.item, stack.count));
        if ours != official {
            self.mismatch(format!(
                "{}: result of {} official {:?}, bamboo {:?}",
                describe(case),
                first.id,
                official,
                ours
            ));
        }
        Ok(())
    }

    fn mismatch(&mut self, message: String) {
        self.mismatches.push(message);
    }

    fn finish(self) -> Result<(), String> {
        let Some((cases, skipped, reasons)) = self.summary else {
            return Err("RecipeOracle.java wrote no summary".to_owned());
        };
        if cases != self.cases {
            return Err(format!(
                "java counted {cases} cases, {} arrived",
                self.cases
            ));
        }
        println!(
            "recipe oracle: {} cases ({} crafting, {} cooking, {} stonecutting, {} smithing, {} brewing), \
             {} skipped, {} differences",
            self.cases,
            self.crafting,
            self.cooking,
            self.stonecutting,
            self.smithing,
            self.brewing,
            skipped,
            self.mismatches.len(),
        );
        if !reasons.is_empty() {
            println!("official skipped cases:");
            for (reason, count) in &reasons {
                println!("  {count} x {reason}");
            }
        }
        for message in self.mismatches.iter().take(self.show) {
            println!("  {message}");
        }
        if self.mismatches.len() > self.show {
            println!(
                "  ... {} more differences",
                self.mismatches.len() - self.show
            );
        }
        if skipped > 0 || !self.mismatches.is_empty() {
            return Err(format!(
                "{} differences, {skipped} skipped",
                self.mismatches.len()
            ));
        }
        Ok(())
    }
}

fn build_input(case: &Line) -> Result<CaseInput, String> {
    match case.kind.as_str() {
        "crafting" => {
            let expected = case.width * case.height;
            if case.slots.len() != expected {
                return Err(format!(
                    "crafting case has {} slots, expected {expected}",
                    case.slots.len()
                ));
            }
            let slots: Vec<ItemStack> = case
                .slots
                .iter()
                .map(|slot| slot.as_ref().map_or(ItemStack::empty(), StackJson::stack))
                .collect();
            Ok(CaseInput::Crafting(CraftingInput::of(
                case.width,
                case.height,
                &slots,
            )))
        }
        "cooking" | "stonecutting" => Ok(CaseInput::Stack(stack(case.input.as_ref()))),
        "smithing" => Ok(CaseInput::Smithing(SmithingInput::new(
            stack(case.template.as_ref()),
            stack(case.base.as_ref()),
            stack(case.addition.as_ref()),
        ))),
        "brewing" => {
            let input = case.input.as_ref();
            let reagent = case.reagent.as_ref();
            Ok(CaseInput::Brewing(BrewingInput::new(
                stack(input),
                input.and_then(|stack| stack.potion),
                stack(reagent),
                reagent.and_then(|stack| stack.potion),
            )))
        }
        other => Err(format!("unknown recipe case kind {other}")),
    }
}

fn stack(stack: Option<&StackJson>) -> ItemStack {
    stack.map_or(ItemStack::empty(), StackJson::stack)
}

fn recipe_type(case: &Line) -> Result<RecipeType, String> {
    match case.kind.as_str() {
        "crafting" => Ok(RecipeType::Crafting),
        "stonecutting" => Ok(RecipeType::Stonecutting),
        "smithing" => Ok(RecipeType::Smithing),
        "brewing" => Ok(RecipeType::Brewing),
        "cooking" => match case.r#type.as_deref() {
            Some("smelting") => Ok(RecipeType::Smelting),
            Some("blasting") => Ok(RecipeType::Blasting),
            Some("smoking") => Ok(RecipeType::Smoking),
            Some("campfire_cooking") => Ok(RecipeType::CampfireCooking),
            other => Err(format!("unknown cooking type {other:?}")),
        },
        other => Err(format!("unknown recipe case kind {other}")),
    }
}

/// 官方 `assemble` 产物的物品和数量。组件不在比较范围内。
fn expected_result(recipe: &Recipe, input: &CaseInput) -> Option<(i32, i32)> {
    match (&recipe.kind, input) {
        // 转化配方沿用匹配到的输入物品；加了材料数的配方，产物数量多出材料数。
        (
            RecipeKind::Transmute {
                input: ingredient,
                result_item,
                result_count,
                add_material_count,
                ..
            },
            CaseInput::Crafting(crafting),
        ) => {
            let mut found = None;
            let mut material = 0;
            for stack in crafting.slots() {
                if stack.is_empty() {
                    continue;
                }
                if ingredient.test(stack.item) {
                    found = Some(stack.item);
                } else {
                    material += 1;
                }
            }
            let item = result_item.unwrap_or(found?);
            let count = if *add_material_count {
                result_count + material
            } else {
                *result_count
            };
            Some((item, count))
        }
        // 修复物品的产物是第一个输入物品，数量 1。
        (RecipeKind::RepairItem, CaseInput::Crafting(crafting)) => Some((
            crafting
                .slots()
                .iter()
                .find(|stack| !stack.is_empty())?
                .item,
            1,
        )),
        // 纹饰的产物是基础物品，数量 1。
        (RecipeKind::SmithingTrim { .. }, CaseInput::Smithing(smithing)) => {
            Some((smithing.base.item, 1))
        }
        _ => recipe
            .result()
            .map(|Stack { item, count, .. }| (item, count)),
    }
}

/// 题目的人能看懂的描述，只打印第一条差异时用。
fn describe(case: &Line) -> String {
    let mut text = case.kind.clone();
    if let Some(kind) = &case.r#type {
        text.push('/');
        text.push_str(kind);
    }
    let stacks = match case.kind.as_str() {
        "crafting" => case
            .slots
            .iter()
            .map(|slot| stack_text(slot.as_ref()))
            .collect::<Vec<_>>()
            .join(" "),
        "smithing" => format!(
            "{} | {} | {}",
            stack_text(case.template.as_ref()),
            stack_text(case.base.as_ref()),
            stack_text(case.addition.as_ref())
        ),
        "brewing" => format!(
            "{} | {}",
            stack_text(case.input.as_ref()),
            stack_text(case.reagent.as_ref())
        ),
        _ => stack_text(case.input.as_ref()),
    };
    format!("{text} [{stacks}]")
}

fn stack_text(stack: Option<&StackJson>) -> String {
    match stack {
        None => "-".to_owned(),
        Some(stack) => match stack.potion {
            Some(potion) => format!("{}x{}p{potion}", stack.item, stack.count),
            None => format!("{}x{}", stack.item, stack.count),
        },
    }
}
