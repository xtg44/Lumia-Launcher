// ============================================================================
// Lumi —— Crystal 插件语言（作者定稿语法）
// 类 Python 缩进式 + 单行括号表达式；事件驱动（listen 控件.事件 / listen 系统事件）；
// 无函数定义、无变量类型、无复杂控制流；动作库白名单即安全边界。
// ============================================================================

use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Num(String),
    Str(String),
    Eq,       // =
    EqEq,     // ==
    NotEq,    // !=
    Bang,     // !
    Colon,    // :
    LParen,   // (
    RParen,   // )
    Dot,      // .
    DotDot,   // ..
    Comma,    // ,
    Plus,     // +
}

#[derive(Debug, Clone)]
struct LineToks {
    indent: usize,
    line: usize,
    toks: Vec<Tok>,
}

fn tokenize(src: &str) -> Result<Vec<LineToks>, String> {
    let mut out = Vec::new();
    for (idx, raw) in src.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw.trim_start_matches([' ', '\t']);
        let indent = raw.len() - trimmed.len();
        let text = trimmed.trim_end();
        if text.is_empty() {
            continue;
        }
        let chars: Vec<char> = text.chars().collect();
        let mut toks = Vec::new();
        let mut i = 0usize;
        while i < chars.len() {
            let c = chars[i];
            if c == '#' || (c == '/' && chars.get(i + 1) == Some(&'/')) {
                break;
            }
            match c {
                '=' if chars.get(i + 1) == Some(&'=') => {
                    toks.push(Tok::EqEq);
                    i += 2;
                }
                '!' if chars.get(i + 1) == Some(&'=') => {
                    toks.push(Tok::NotEq);
                    i += 2;
                }
                '=' => {
                    toks.push(Tok::Eq);
                    i += 1;
                }
                '!' => {
                    toks.push(Tok::Bang);
                    i += 1;
                }
                ':' => {
                    toks.push(Tok::Colon);
                    i += 1;
                }
                '(' => {
                    toks.push(Tok::LParen);
                    i += 1;
                }
                ')' => {
                    toks.push(Tok::RParen);
                    i += 1;
                }
                '.' if chars.get(i + 1) == Some(&'.') => {
                    toks.push(Tok::DotDot);
                    i += 2;
                }
                '.' => {
                    toks.push(Tok::Dot);
                    i += 1;
                }
                ',' => {
                    toks.push(Tok::Comma);
                    i += 1;
                }
                '+' => {
                    toks.push(Tok::Plus);
                    i += 1;
                }
                '\'' | '"' => {
                    let quote = c;
                    let mut buf = String::new();
                    i += 1;
                    let mut closed = false;
                    while i < chars.len() {
                        if chars[i] == quote {
                            closed = true;
                            i += 1;
                            break;
                        }
                        buf.push(chars[i]);
                        i += 1;
                    }
                    if !closed {
                        return Err(format!("第 {} 行：字符串没有闭合引号", line_no));
                    }
                    toks.push(Tok::Str(buf));
                }
                c if c.is_ascii_digit() => {
                    let mut buf = String::new();
                    while i < chars.len()
                        && chars[i].is_ascii_digit()
                        && !(chars[i] == '.' && chars.get(i + 1) == Some(&'.'))
                    {
                        buf.push(chars[i]);
                        i += 1;
                    }
                    toks.push(Tok::Num(buf));
                }
                c if c.is_alphanumeric() || c == '_' || c == '-' => {
                    let mut buf = String::new();
                    while i < chars.len()
                        && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-')
                    {
                        buf.push(chars[i]);
                        i += 1;
                    }
                    toks.push(Tok::Ident(buf));
                }
                c if c.is_whitespace() => {
                    i += 1;
                }
                _ => {
                    return Err(format!("第 {} 行：无法识别的字符 '{}'", line_no, c));
                }
            }
        }
        if toks.is_empty() {
            continue;
        }
        out.push(LineToks { indent, line: line_no, toks });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ControlKind {
    Button(String),
    Text(String),
    Input(String),
    List(Vec<String>),
    Toggle(String),
    Image(String),
    Line(Option<i64>, bool), // (长度, portrait)
}

#[derive(Debug, Clone)]
pub enum Expr {
    Str(String),
    Num(f64),
    ControlText(String),
    Selected(String),
    /// 控件的当前值（输入框内容 / 滑块位置），`X.value`
    Value(String),
    /// 字符串拼接（`'共 ' + count + ' 个'`）；数字会自动转成文本
    Concat(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Cond {
    Eq(Expr, Expr),
    Ne(Expr, Expr),
    IsSelect(String),
    NotSelect(String),
}

#[derive(Debug, Clone)]
pub enum IterExpr {
    Control(String),
    List(Vec<String>),
    Range(i64, i64),
    RangeStep(i64, i64, i64),
}

#[derive(Debug, Clone)]
pub enum Action {
    Copy(String, String),
    Move(String, String),
    Delete(String),
    Open(String),
    Popup(Expr),
    Toast(Expr),
    Sleep(f64),
    LaunchGame,
    Mkdir(String),       // mkdir 路径 —— 创建目录（可多级）
    Exists(String),      // exists 路径 —— 判断存在，用 toast 反馈
    ListFiles(String),   // listFiles 路径 —— 列出目录内容，用 toast 反馈

    // ---- v1.1 起新增 ----
    WriteFile(String, Expr),      // writeFile 路径 内容 —— 覆盖写入（自动建父目录）
    AppendFile(String, Expr),     // appendFile 路径 内容 —— 追加
    ReadFile(String),             // readFile 路径 —— 读文本，结果用 toast 反馈（超长截断）
    Rename(String, String),       // rename 旧 to 新 —— 重命名/同盘移动单个文件
    CopyText(Expr),               // copyText 文字 —— 复制到系统剪贴板
    Log(Expr),                    // log 文字 —— 追加到插件目录的 plugin.log
    Zip(String, String),          // zip 源 to 目标.zip
    Unzip(String, String),        // unzip 源.zip to 目标目录
    Notify(Expr),                 // notify 文字 —— 常驻提示（比 toast 久）
}

#[derive(Debug, Clone)]
pub enum ListenerTarget {
    Control(String),
    System(String),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Meta(String, String),
    Create(String, ControlKind), // 控件名, 控件种类
    Layout(String, String),
    AssignControl(String, Expr),
    Listen(ListenerTarget, String, Vec<Stmt>),
    If(Cond, Vec<Stmt>, Vec<Stmt>),
    While(Vec<Stmt>),
    For(String, IterExpr, Vec<Stmt>),
    Break,
    Action(Action),
    /// `set 名字 = 表达式` —— 写进插件自己的变量表（跨事件保留）
    SetVar(String, Expr),
    Page(String, Vec<Stmt>),
}

#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: String,
    pub min: String,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    lines: Vec<LineToks>,
    idx: usize,
}

fn ident(t: &Tok) -> Option<&str> {
    match t {
        Tok::Ident(s) => Some(s),
        _ => None,
    }
}

impl Parser {
    fn new(lines: Vec<LineToks>) -> Self {
        Parser { lines, idx: 0 }
    }

    fn line_no(&self) -> usize {
        self.lines.get(self.idx).map(|l| l.line).unwrap_or(0)
    }

    fn err(&self, msg: &str) -> String {
        format!("第 {} 行：{}", self.line_no(), msg)
    }

    fn peek_indent(&self) -> usize {
        self.lines.get(self.idx).map(|l| l.indent).unwrap_or(0)
    }

    fn block(&mut self, base: usize) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while self.idx < self.lines.len() && self.lines[self.idx].indent > base {
            let indent = self.lines[self.idx].indent;
            stmts.push(self.stmt(indent)?);
        }
        Ok(stmts)
    }

    fn stmt(&mut self, indent: usize) -> Result<Stmt, String> {
        let toks = self
            .lines
            .get(self.idx)
            .ok_or_else(|| self.err("意外结束"))?
            .toks
            .clone();
        let last = toks.last().cloned();

        // 单行括号式：if 条件(动作) / listen 目标.事件(动作)
        if matches!(last, Some(Tok::RParen))
            && matches!(toks.first(), Some(Tok::Ident(k)) if k == "if" || k == "listen")
        {
            return self.single_line_stmt(indent);
        }

        // 块式：行尾 :
        if matches!(last, Some(Tok::Colon)) {
            match toks.first() {
                Some(Tok::Ident(k)) if k == "if" => return self.if_stmt(indent, &toks),
                Some(Tok::Ident(k)) if k == "listen" => return self.listen_stmt(indent, &toks),
                Some(Tok::Ident(k)) if k == "while" => {
                    self.advance();
                    let body = self.block(indent)?;
                    return Ok(Stmt::While(body));
                }
                Some(Tok::Ident(k)) if k == "for" => return self.for_stmt(indent, &toks),
                Some(Tok::Ident(k)) if k == "page" => {
                    let page_name = toks
                        .get(1)
                        .and_then(|t| ident(t))
                        .ok_or_else(|| self.err("page 缺少页面名称"))?
                        .to_string();
                    self.advance();
                    let body = self.block(indent)?;
                    return Ok(Stmt::Page(page_name, body));
                }
                _ => {}
            }
        }

        self.advance();
        self.plain_stmt(&toks)
    }

    fn advance(&mut self) {
        self.idx += 1;
    }

    // ---- 单行括号式 ----
    fn single_line_stmt(&mut self, _indent: usize) -> Result<Stmt, String> {
        let toks = self.lines[self.idx].toks.clone();
        let kw = ident(toks.first().unwrap()).unwrap().to_string();
        let open = toks
            .iter()
            .position(|t| matches!(t, Tok::LParen))
            .ok_or_else(|| self.err("缺少 '('"))?;
        let head = &toks[1..open];
        let inner = &toks[open + 1..];
        if !matches!(inner.last(), Some(Tok::RParen)) {
            return Err(self.err("缺少 ')'"));
        }
        let body_toks = &inner[..inner.len() - 1];
        let body = self.inline_stmts(body_toks)?;
        self.advance();
        if kw == "if" {
            let cond = self.cond_from_toks(head)?;
            Ok(Stmt::If(cond, body, vec![]))
        } else {
            let (target, event) = self.listener_from_toks(head)?;
            Ok(Stmt::Listen(target, event, body))
        }
    }

    /// 括号体内以逗号或分号分隔的语句。目前支持：赋值 X = '...' / 单动作。
    /// tokenizer 没产生 Semicolon，但用户示例用单条赋值；这里逗号分隔多动作。
    fn inline_stmts(&self, toks: &[Tok]) -> Result<Vec<Stmt>, String> {
        if toks.is_empty() {
            return Ok(vec![]);
        }
        // 用逗号切分为多个「语句片段」，每个片段都当作一行来解析
        let mut parts: Vec<Vec<Tok>> = Vec::new();
        let mut cur: Vec<Tok> = Vec::new();
        for t in toks {
            if matches!(t, Tok::Comma) || matches!(t, Tok::Comma) {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
            } else {
                cur.push(t.clone());
            }
        }
        if !cur.is_empty() {
            parts.push(cur);
        }
        let mut stmts = Vec::new();
        for part in parts {
            let ptoks = part;
            let line_no = self.line_no();
            let mut p = Parser::new(vec![LineToks { indent: 1, line: line_no, toks: ptoks.clone() }]);
            stmts.push(p.plain_stmt(&ptoks)?);
        }
        Ok(stmts)
    }

    // ---- 块式 ----
    fn if_stmt(&mut self, indent: usize, toks: &[Tok]) -> Result<Stmt, String> {
        let head = &toks[1..toks.len() - 1]; // 去掉 if 和 :
        let cond = self.cond_from_toks(head)?;
        self.advance();
        let body = self.block(indent)?;
        let mut else_body = Vec::new();
        if self.idx < self.lines.len()
            && self.lines[self.idx].indent == indent
            && ident(self.lines[self.idx].toks.first().unwrap_or(&Tok::Ident(String::new())))
                == Some("else")
        {
            self.advance();
            else_body = self.block(indent)?;
        }
        Ok(Stmt::If(cond, body, else_body))
    }

    fn listen_stmt(&mut self, indent: usize, toks: &[Tok]) -> Result<Stmt, String> {
        let head = &toks[1..toks.len() - 1];
        let (target, event) = self.listener_from_toks(head)?;
        self.advance();
        let body = self.block(indent)?;
        Ok(Stmt::Listen(target, event, body))
    }

    fn for_stmt(&mut self, indent: usize, toks: &[Tok]) -> Result<Stmt, String> {
        let head = &toks[1..toks.len() - 1]; // for ... :
        let var = ident(head.first().ok_or_else(|| self.err("for 缺少变量"))?)
            .ok_or_else(|| self.err("for 变量名非法"))?
            .to_string();
        let in_pos = head
            .iter()
            .position(|t| ident(t) == Some("in"))
            .ok_or_else(|| self.err("for 缺少 in"))?;
        let iter = self.iter_from_toks(&head[in_pos + 1..])?;
        self.advance();
        let body = self.block(indent)?;
        Ok(Stmt::For(var, iter, body))
    }

    fn iter_from_toks(&self, toks: &[Tok]) -> Result<IterExpr, String> {
        if toks.is_empty() {
            return Err(self.err("for 缺少迭代对象"));
        }
        // 区间（数字或控件起点）优先判断：a..b [step n]
        if matches!(toks.get(1), Some(Tok::DotDot)) && toks.len() >= 3 {
            let a = toks[0]
                .clone()
                .as_num_i64()
                .ok_or_else(|| self.err("区间起点不是数字"))?;
            let b = toks[2]
                .clone()
                .as_num_i64()
                .ok_or_else(|| self.err("区间终点不是数字"))?;
            if toks.len() >= 5 && ident(&toks[3]) == Some("step") {
                let n = toks[4]
                    .clone()
                    .as_num_i64()
                    .ok_or_else(|| self.err("步长不是数字"))?;
                return Ok(IterExpr::RangeStep(a, b, n));
            }
            return Ok(IterExpr::Range(a, b));
        }
        if let Some(name) = ident(&toks[0]) {
            if toks.len() == 1 {
                return Ok(IterExpr::Control(name.to_string()));
            }
        }
        if matches!(toks.first(), Some(Tok::LParen)) {
            let mut items = Vec::new();
            for t in toks.iter().skip(1) {
                match t {
                    Tok::Str(s) => items.push(s.clone()),
                    Tok::Comma | Tok::RParen => {}
                    _ => return Err(self.err("列表字面量只允许字符串")),
                }
            }
            return Ok(IterExpr::List(items));
        }
        Err(self.err("无法识别的 for 迭代对象"))
    }

    fn cond_from_toks(&self, toks: &[Tok]) -> Result<Cond, String> {
        // 形式：A == B / A != B / X is select / X !is select
        if toks.len() == 3 && ident(&toks[1]) == Some("is") {
            return Ok(Cond::IsSelect(
                ident(&toks[0]).ok_or_else(|| self.err("is select 左边必须是控件"))?.to_string(),
            ));
        }
        if toks.len() == 4 && matches!(toks[0], Tok::Bang) && ident(&toks[2]) == Some("is") {
            return Ok(Cond::NotSelect(
                ident(&toks[1]).ok_or_else(|| self.err("!is select 左边必须是控件"))?.to_string(),
            ));
        }
        let op_pos = toks
            .iter()
            .position(|t| matches!(t, Tok::EqEq | Tok::NotEq))
            .ok_or_else(|| self.err("条件缺少 == / !="))?;
        let left = self.expr_from_toks(&toks[..op_pos])?;
        let right = self.expr_from_toks(&toks[op_pos + 1..])?;
        if matches!(toks[op_pos], Tok::NotEq) {
            Ok(Cond::Ne(left, right))
        } else {
            Ok(Cond::Eq(left, right))
        }
    }

    fn listener_from_toks(&self, head: &[Tok]) -> Result<(ListenerTarget, String), String> {
        if matches!(head.get(1), Some(Tok::Dot)) {
            let c = ident(&head[0]).ok_or_else(|| self.err("listen 控件名非法"))?.to_string();
            let e = ident(&head[2]).ok_or_else(|| self.err("listen 事件名非法"))?.to_string();
            Ok((ListenerTarget::Control(c), e))
        } else {
            let e = ident(&head[0]).ok_or_else(|| self.err("listen 事件名非法"))?.to_string();
            Ok((ListenerTarget::System(e), "".to_string()))
        }
    }

    /// 表达式 = 若干个原子用 `+` 连起来（左结合）。`+` 只做字符串拼接。
    fn expr_from_toks(&self, toks: &[Tok]) -> Result<Expr, String> {
        if toks.is_empty() {
            return Err(self.err("表达式为空"));
        }
        let mut parts: Vec<&[Tok]> = Vec::new();
        let mut start = 0usize;
        for (i, t) in toks.iter().enumerate() {
            if *t == Tok::Plus {
                parts.push(&toks[start..i]);
                start = i + 1;
            }
        }
        parts.push(&toks[start..]);
        let mut out: Option<Expr> = None;
        for part in parts {
            let atom = self.atom_from_toks(part)?;
            out = Some(match out {
                None => atom,
                Some(prev) => Expr::Concat(Box::new(prev), Box::new(atom)),
            });
        }
        out.ok_or_else(|| self.err("表达式为空"))
    }

    fn atom_from_toks(&self, toks: &[Tok]) -> Result<Expr, String> {
        let t = toks.first().ok_or_else(|| self.err("+ 两侧都要有内容"))?;
        match t {
            Tok::Str(s) => Ok(Expr::Str(s.clone())),
            Tok::Num(n) => Ok(Expr::Num(n.parse().unwrap_or(0.0))),
            Tok::Ident(name) => {
                if toks.len() >= 3 && matches!(toks.get(1), Some(Tok::Dot)) {
                    match toks.get(2) {
                        Some(Tok::Ident(s)) if s == "selected" => {
                            return Ok(Expr::Selected(name.clone()))
                        }
                        Some(Tok::Ident(s)) if s == "value" => {
                            return Ok(Expr::Value(name.clone()))
                        }
                        _ => {}
                    }
                }
                Ok(Expr::ControlText(name.clone()))
            }
            _ => Err(self.err("表达式只支持字符串/数字/控件")),
        }
    }

    fn plain_stmt(&mut self, toks: &[Tok]) -> Result<Stmt, String> {
        let first = ident(toks.first().unwrap_or(&Tok::Ident(String::new())))
            .unwrap_or("")
            .to_string();
        match first.as_str() {
            "break" => return Ok(Stmt::Break),
            "name" | "version" | "description" | "icon" | "min" => {
                if let Some(Tok::Colon) = toks.get(1) {
                    // 冒号后的全部 token 拼接（支持裸词多词：`name: Auto Backup`），
                    // 带引号则原样取字符串内容
                    let v = toks[2..]
                        .iter()
                        .map(|t| match t {
                            Tok::Str(s) => s.clone(),
                            Tok::Ident(s) => s.clone(),
                            Tok::Num(n) => n.clone(),
                            _ => String::new(),
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    return Ok(Stmt::Meta(first, v.trim().to_string()));
                }
            }
            // set 名字 = 表达式
            "set" => {
                if !matches!(toks.get(2), Some(Tok::Eq)) {
                    return Err(self.err("set 语法应为：set 名字 = 表达式"));
                }
                let name = ident(toks.get(1).ok_or_else(|| self.err("set 缺少名字"))?)
                    .ok_or_else(|| self.err("set 后面的名字不合法"))?
                    .to_string();
                let expr = self.expr_from_toks(&toks[3..])?;
                return Ok(Stmt::SetVar(name, expr));
            }
            "copy" | "move" | "delete" | "open" | "popup" | "toast" | "sleep" | "launchGame"
            | "mkdir" | "exists" | "listFiles" | "writeFile" | "appendFile" | "readFile"
            | "rename" | "copyText" | "log" | "zip" | "unzip" | "notify" => {
                return Ok(Stmt::Action(self.action_from_toks(toks)?));
            }
            _ => {}
        }
        // X = CreateXxx(...)
        if matches!(toks.get(1), Some(Tok::Eq)) {
            let name = first.clone();
            let rhs = &toks[2..];
            if let Some(Tok::Ident(f)) = rhs.first() {
                if f.starts_with("Create") {
                    let kind = self.control_kind(rhs)?;
                    return Ok(Stmt::Create(name, kind));
                }
            }
            let expr = self.expr_from_toks(rhs)?;
            return Ok(Stmt::AssignControl(name, expr));
        }
        // X.left / X.center / X.right
        if let Some(Tok::Dot) = toks.get(1) {
            if let Some(Tok::Ident(pos)) = toks.get(2) {
                if ["left", "center", "right", "top", "bottom"].contains(&pos.as_str()) {
                    return Ok(Stmt::Layout(first, pos.clone()));
                }
            }
        }
        Err(format!("第 {} 行：无法识别的语句", self.line_no()))
    }

    fn action_from_toks(&self, toks: &[Tok]) -> Result<Action, String> {
        let kw = ident(toks.first().unwrap()).unwrap().to_string();
        match kw.as_str() {
            "copy" | "move" => {
                let a = self.path_arg(toks.get(1), "copy/move 缺少来源")?;
                let to_pos = toks
                    .iter()
                    .position(|t| ident(t) == Some("to"))
                    .ok_or_else(|| self.err("copy/move 缺少 to"))?;
                let b = self.path_arg(toks.get(to_pos + 1), "copy/move 缺少目标")?;
                Ok(if kw == "copy" { Action::Copy(a, b) } else { Action::Move(a, b) })
            }
            "delete" => Ok(Action::Delete(self.path_arg(toks.get(1), "delete 缺少目标")?)),
            "open" => Ok(Action::Open(self.path_arg(toks.get(1), "open 缺少目标")?)),
            "popup" => Ok(Action::Popup(self.rest_expr(toks, 1)?)),
            "toast" => Ok(Action::Toast(self.rest_expr(toks, 1)?)),
            "sleep" => {
                let n: f64 = toks
                    .get(1)
                    .and_then(|t| match t {
                        Tok::Num(s) => s.parse().ok(),
                        _ => None,
                    })
                    .ok_or_else(|| self.err("sleep 缺少秒数"))?;
                Ok(Action::Sleep(n))
            }
            "launchGame" => Ok(Action::LaunchGame),
            "mkdir" => Ok(Action::Mkdir(self.path_arg(toks.get(1), "mkdir 缺少路径")?)),
            "exists" => Ok(Action::Exists(self.path_arg(toks.get(1), "exists 缺少路径")?)),
            "listFiles" => Ok(Action::ListFiles(self.path_arg(toks.get(1), "listFiles 缺少路径")?)),
            // ---- v1.1 新增 ----
            "writeFile" | "appendFile" => {
                let path = self.path_arg(toks.get(1), "缺少文件路径")?;
                let text = self.rest_expr(toks, 2)?;
                Ok(if kw == "writeFile" {
                    Action::WriteFile(path, text)
                } else {
                    Action::AppendFile(path, text)
                })
            }
            "readFile" => Ok(Action::ReadFile(self.path_arg(toks.get(1), "readFile 缺少路径")?)),
            "rename" => {
                // rename 旧 to 新
                if !matches!(toks.get(2), Some(Tok::Ident(t)) if t == "to") {
                    return Err(self.err("rename 语法应为：rename 旧 to 新"));
                }
                Ok(Action::Rename(
                    self.path_arg(toks.get(1), "rename 缺少原路径")?,
                    self.path_arg(toks.get(3), "rename 缺少新路径")?,
                ))
            }
            "copyText" => Ok(Action::CopyText(self.rest_expr(toks, 1)?)),
            "log" => Ok(Action::Log(self.rest_expr(toks, 1)?)),
            "notify" => Ok(Action::Notify(self.rest_expr(toks, 1)?)),
            "zip" | "unzip" => {
                let src = self.path_arg(toks.get(1), "缺少源路径")?;
                if !matches!(toks.get(2), Some(Tok::Ident(t)) if t == "to") {
                    return Err(self.err(&format!("{} 语法应为：{} 源 to 目标", kw, kw)));
                }
                let dst = self.path_arg(toks.get(3), "缺少目标路径")?;
                Ok(if kw == "zip" {
                    Action::Zip(src, dst)
                } else {
                    Action::Unzip(src, dst)
                })
            }
            _ => Err(self.err("未知动作")),
        }
    }

    fn path_arg(&self, t: Option<&Tok>, msg: &str) -> Result<String, String> {
        match t {
            Some(Tok::Str(s)) | Some(Tok::Ident(s)) => Ok(s.clone()),
            _ => Err(self.err(msg)),
        }
    }

    /// 把 from 之后的 token 当作一个表达式（运行时求值）：
    /// `notify '共 ' + count + ' 个'` 这种拼接也支持。
    fn rest_expr(&self, toks: &[Tok], from: usize) -> Result<Expr, String> {
        let mut slice = &toks[from.min(toks.len())..];
        // 兼容括号式：toast('文字')
        if matches!(slice.first(), Some(Tok::LParen)) {
            slice = &slice[1..];
        }
        let mut end = slice.len();
        if end > 0 && matches!(slice[end - 1], Tok::RParen) {
            end -= 1;
        }
        let slice = &slice[..end];
        if slice.is_empty() {
            return Err(self.err("缺少文字内容"));
        }
        self.expr_from_toks(slice)
    }

    fn str_arg(&self, t: Option<&Tok>) -> Option<String> {
        match t {
            Some(Tok::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }

    fn control_kind(&self, rhs: &[Tok]) -> Result<ControlKind, String> {
        let f = ident(rhs.first().unwrap()).unwrap().to_string();
        // args：提取字符串/数字/portrait
        let arg_toks = &rhs[1..];
        let mut args: Vec<Tok> = Vec::new();
        for t in arg_toks {
            match t {
                Tok::Str(s) => args.push(Tok::Str(s.clone())),
                Tok::Num(n) => args.push(Tok::Num(n.clone())),
                Tok::Ident(s) if s == "portrait" => args.push(Tok::Ident("portrait".to_string())),
                _ => {}
            }
        }
        match f.as_str() {
            "CreateButton" => Ok(ControlKind::Button(self.first_str(&args).unwrap_or_default())),
            "CreateText" => Ok(ControlKind::Text(self.first_str(&args).unwrap_or_default())),
            "CreateInput" => Ok(ControlKind::Input(self.first_str(&args).unwrap_or_default())),
            "CreateList" => {
                let items = args
                    .iter()
                    .filter_map(|t| match t {
                        Tok::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                Ok(ControlKind::List(items))
            }
            "CreateToggle" => Ok(ControlKind::Toggle(self.first_str(&args).unwrap_or_default())),
            "CreateImage" => Ok(ControlKind::Image(self.first_str(&args).unwrap_or_default())),
            "CreateLine" => {
                let len = args.first().and_then(|t| match t {
                    Tok::Num(n) => n.parse().ok(), // 兼容 i64 解析（f64 会失败的情况）
                    _ => None,
                });
                let is_portrait = args
                    .iter()
                    .any(|t| matches!(t, Tok::Ident(s) if s == "portrait"));
                let len_i64 = len.or_else(|| {
                    args.first().and_then(|t| match t {
                        Tok::Num(n) => n.parse::<f64>().ok().map(|f| f as i64),
                        _ => None,
                    })
                });
                Ok(ControlKind::Line(len_i64, is_portrait))
            }
            _ => Err(self.err(&format!("未知控件 {}", f))),
        }
    }

    fn first_str(&self, args: &[Tok]) -> Option<String> {
        args.first().and_then(|t| match t {
            Tok::Str(s) => Some(s.clone()),
            _ => None,
        })
    }

    pub fn parse(mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while self.idx < self.lines.len() {
            let indent = self.peek_indent();
            stmts.push(self.stmt(indent)?);
        }
        Ok(stmts)
    }
}

trait AsNumI64 {
    fn as_num_i64(self) -> Option<i64>;
}
impl AsNumI64 for Tok {
    fn as_num_i64(self) -> Option<i64> {
        match self {
            Tok::Num(s) => s.parse::<f64>().ok().map(|f| f as i64),
            _ => None,
        }
    }
}

pub fn parse_lumi(src: &str) -> Result<Vec<Stmt>, String> {
    let lines = tokenize(src)?;
    Parser::new(lines).parse()
}

// ---------------------------------------------------------------------------
// 运行时
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ControlNode {
    pub name: String,
    pub ctype: String, // button/text/input/list/toggle/image/line
    pub text: String,
    pub layout: String,
    pub list: Vec<String>,
    pub checked: bool,
    pub selected: Option<String>,
    /// 控件的当前值（输入框内容 / 滑块位置），由前端在触发事件时回传
    pub value: Option<String>,
    pub line_len: Option<i64>,
    pub portrait: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Notify {
    pub kind: String, // toast / popup
    pub text: String,
}

#[derive(Debug)]
pub struct PluginRuntime {
    pub meta: Meta,
    pub controls: Vec<ControlNode>,
    pub handlers: Vec<(ListenerTarget, String, Vec<Stmt>)>,
    /// 顶层 while 的循环体（每轮由独立后台任务执行一遍）
    pub while_blocks: Vec<Vec<Stmt>>,
    /// 顶层 if 的持续评估块（每轮由 step_loops 评估）
    pub loops: Vec<Vec<Stmt>>,
    pub plugin_dir: PathBuf,
    pub last_error: Option<String>,
    /// 页面注入：page_name -> controls
    pub page_injects: HashMap<String, Vec<ControlNode>>,
    /// 页面控件覆盖：page_name -> { control_name -> text }
    pub page_overrides: HashMap<String, HashMap<String, String>>,
    /// 插件自己的变量（`set 名字 = 表达式`），跨事件保留
    pub vars: HashMap<String, String>,
}

impl PluginRuntime {
    fn control_mut(&mut self, name: &str) -> Option<&mut ControlNode> {
        self.controls.iter_mut().find(|c| c.name == name)
    }

    fn control(&self, name: &str) -> Option<&ControlNode> {
        self.controls.iter().find(|c| c.name == name)
    }

    fn resolve_path(&self, s: &str) -> PathBuf {
        match s {
            "game-saves" => self.data_root().join("saves"),
            "backup-folder" => self.plugin_dir.join("backup"),
            "game-dir" => self.data_root(),
            "plugin-dir" => self.plugin_dir.clone(),
            "versions-dir" => self.data_root().join("versions"),
            "launcher-dir" => self.data_root(),
            "music-dir" => self.data_root().join("music_library"),
            "today" => PathBuf::from(format!("{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs())),
            // 其他路径：绝对路径原样；相对路径基于插件自身目录（插件操作自己的数据，不污染 cwd）
            _ => {
                let p = PathBuf::from(s);
                if p.is_absolute() {
                    p
                } else {
                    self.plugin_dir.join(p)
                }
            }
        }
    }

    // 数据根目录（插件数据旁的启动器数据区）
    fn data_root(&self) -> PathBuf {
        self.plugin_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.plugin_dir.clone())
    }
}

pub fn build_runtime(src: &str, plugin_dir: PathBuf) -> Result<PluginRuntime, String> {
    let stmts = parse_lumi(src)?;
    // 收集 meta
    let mut meta = Meta::default();
    for s in &stmts {
        if let Stmt::Meta(k, v) = s {
            match k.as_str() {
                "name" => meta.name = v.clone(),
                "version" => meta.version = v.clone(),
                "description" => meta.description = v.clone(),
                "icon" => meta.icon = v.clone(),
                "min" => meta.min = v.clone(),
                _ => {}
            }
        }
    }
    let mut rt = PluginRuntime {
        meta,
        controls: Vec::new(),
        handlers: Vec::new(),
        while_blocks: Vec::new(),
        loops: Vec::new(),
        plugin_dir,
        last_error: None,
        page_injects: HashMap::new(),
        page_overrides: HashMap::new(),
        vars: HashMap::new(),
    };
    // 分派顶层语句
    for s in &stmts {
        match s {
            Stmt::Create(name, kind) => {
                let (ctype, text, list, line_len, portrait) = match kind {
                    ControlKind::Button(t) => ("button", t.clone(), vec![], None, false),
                    ControlKind::Text(t) => ("text", t.clone(), vec![], None, false),
                    ControlKind::Input(t) => ("input", t.clone(), vec![], None, false),
                    ControlKind::List(items) => ("list", String::new(), items.clone(), None, false),
                    ControlKind::Toggle(t) => ("toggle", t.clone(), vec![], None, false),
                    ControlKind::Image(t) => ("image", t.clone(), vec![], None, false),
                    ControlKind::Line(len, p) => ("line", String::new(), vec![], *len, *p),
                };
                rt.controls.push(ControlNode {
                    name: name.clone(),
                    ctype: ctype.to_string(),
                    text,
                    layout: "left".to_string(),
                    list,
                    checked: false,
                    selected: None,
                    value: None,
                    line_len,
                    portrait,
                });
            }
            Stmt::Layout(name, pos) => {
                if let Some(c) = rt.control_mut(name) {
                    c.layout = pos.clone();
                }
            }
            Stmt::AssignControl(name, expr) => {
                let val = eval_expr(expr, &rt, &HashMap::new()).unwrap_or_default();
                if let Some(c) = rt.control_mut(name) {
                    if matches!(c.ctype.as_str(), "text" | "button" | "input" | "toggle") {
                        c.text = val;
                    }
                }
            }
            Stmt::Listen(t, ev, body) => {
                rt.handlers.push((t.clone(), ev.clone(), body.clone()));
            }
            Stmt::While(body) => {
                // 顶层 while：由独立后台任务每轮执行 body 一遍（build_while_blocks 由外部 spawn）
                rt.while_blocks.push(body.clone());
            }
            Stmt::If(cond, body, else_body) => {
                rt.loops.push(vec![Stmt::If(cond.clone(), body.clone(), else_body.clone())]);
            }
            Stmt::Page(page_name, body) => {
                let mut page_ctls: Vec<ControlNode> = Vec::new();
                let mut overrides: HashMap<String, String> = HashMap::new();
                for ps in body {
                    match ps {
                        Stmt::Create(name, kind) => {
                            let (ctype, text, list, line_len, portrait) = match kind {
                                ControlKind::Button(t) => ("button", t.clone(), vec![], None, false),
                                ControlKind::Text(t) => ("text", t.clone(), vec![], None, false),
                                ControlKind::Input(t) => ("input", t.clone(), vec![], None, false),
                                ControlKind::List(items) => ("list", String::new(), items.clone(), None, false),
                                ControlKind::Toggle(t) => ("toggle", t.clone(), vec![], None, false),
                                ControlKind::Image(t) => ("image", t.clone(), vec![], None, false),
                                ControlKind::Line(len, p) => ("line", String::new(), vec![], *len, *p),
                            };
                            page_ctls.push(ControlNode {
                                name: name.clone(),
                                ctype: ctype.to_string(),
                                text,
                                layout: "left".to_string(),
                                list,
                                checked: false,
                                selected: None,
                                value: None,
                                line_len,
                                portrait,
                            });
                        }
                        Stmt::Layout(name, pos) => {
                            if let Some(c) = page_ctls.iter_mut().find(|c| c.name == *name) {
                                c.layout = pos.clone();
                            }
                        }
                        Stmt::AssignControl(name, expr) => {
                            if let Ok(val) = eval_expr(expr, &rt, &HashMap::new()) {
                                overrides.insert(name.clone(), val);
                            }
                        }
                        _ => {}
                    }
                }
                rt.page_injects.insert(page_name.clone(), page_ctls);
            }
            _ => {}
        }
    }
    Ok(rt)
}

fn eval_expr(e: &Expr, rt: &PluginRuntime, ctx: &HashMap<String, String>) -> Result<String, String> {
    Ok(match e {
        Expr::Str(s) => s.clone(),
        Expr::Num(n) => {
            if n.fract() == 0.0 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        // 标识符：循环变量 → 插件变量 → 控件文字
        Expr::ControlText(n) => ctx
            .get(n)
            .cloned()
            .or_else(|| rt.vars.get(n).cloned())
            .or_else(|| rt.control(n).map(|c| c.text.clone()))
            .unwrap_or_default(),
        // 控件当前值：输入框取输入内容，滑块取位置，其余退回文字
        Expr::Value(n) => rt
            .control(n)
            .map(|c| c.value.clone().unwrap_or_else(|| c.text.clone()))
            .unwrap_or_default(),
        Expr::Concat(a, b) => format!("{}{}", eval_expr(a, rt, ctx)?, eval_expr(b, rt, ctx)?),
        Expr::Selected(n) => rt
            .control(n)
            .and_then(|c| c.selected.clone())
            .unwrap_or_default(),
    })
}

fn eval_cond(c: &Cond, rt: &PluginRuntime, ctx: &HashMap<String, String>) -> Result<bool, String> {
    Ok(match c {
        Cond::Eq(a, b) => eval_expr(a, rt, ctx)? == eval_expr(b, rt, ctx)?,
        Cond::Ne(a, b) => eval_expr(a, rt, ctx)? != eval_expr(b, rt, ctx)?,
        Cond::IsSelect(n) => rt.control(n).map(|c| c.selected.is_some()).unwrap_or(false),
        Cond::NotSelect(n) => rt.control(n).map(|c| c.selected.is_none()).unwrap_or(true),
    })
}

/// Box 包装：断开 async fn 直接递归（E0733），递归点统一走这里。
fn box_exec<'a>(
    rt: &'a Arc<Mutex<PluginRuntime>>,
    stmts: &'a [Stmt],
    notify: &'a mut Vec<Notify>,
    ctx: &'a mut HashMap<String, String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(exec_block(rt, stmts, notify, ctx))
}

fn box_pass<'a>(
    rt: &'a Arc<Mutex<PluginRuntime>>,
    stmts: &'a [Stmt],
    notify: &'a mut Vec<Notify>,
    ctx: &'a mut HashMap<String, String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(exec_block_pass(rt, stmts, notify, ctx))
}

/// 执行语句块（async：copy/move/sleep 需要）。
async fn exec_block(
    rt: &Arc<Mutex<PluginRuntime>>,
    stmts: &[Stmt],
    notify: &mut Vec<Notify>,
    ctx: &mut HashMap<String, String>,
) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            Stmt::Meta(_, _) => {}
            Stmt::Create(_, _) | Stmt::Layout(_, _) => {
                // 运行时动态创建/布局（listen 块内少见），按顶层逻辑处理
                let mut guard = rt.lock().unwrap();
                if let Stmt::Create(name, kind) = stmt {
                    let (ctype, text, list, line_len, portrait) = match kind {
                        ControlKind::Button(t) => ("button", t.clone(), vec![], None, false),
                        ControlKind::Text(t) => ("text", t.clone(), vec![], None, false),
                        ControlKind::Input(t) => ("input", t.clone(), vec![], None, false),
                        ControlKind::List(items) => {
                            ("list", String::new(), items.clone(), None, false)
                        }
                        ControlKind::Toggle(t) => ("toggle", t.clone(), vec![], None, false),
                        ControlKind::Image(t) => ("image", t.clone(), vec![], None, false),
                        ControlKind::Line(len, p) => ("line", String::new(), vec![], *len, *p),
                    };
                    guard.controls.push(ControlNode {
                        name: name.clone(),
                        ctype: ctype.to_string(),
                        text,
                        layout: "left".to_string(),
                        list,
                        checked: false,
                        selected: None,
                        value: None,
                        line_len,
                        portrait,
                    });
                } else if let Stmt::Layout(name, pos) = stmt {
                    if let Some(c) = guard.control_mut(name) {
                        c.layout = pos.clone();
                    }
                }
                drop(guard);
            }
            Stmt::AssignControl(name, expr) => {
                let val = {
                    let guard = rt.lock().unwrap();
                    eval_expr(expr, &guard, ctx).unwrap_or_default()
                };
                {
                    let mut guard = rt.lock().unwrap();
                    if let Some(c) = guard.control_mut(name) {
                        if matches!(c.ctype.as_str(), "text" | "button" | "input" | "toggle") {
                            c.text = val;
                        }
                    }
                }
            }
            Stmt::SetVar(name, expr) => {
                let val = {
                    let guard = rt.lock().unwrap();
                    eval_expr(expr, &guard, ctx)?
                };
                rt.lock().unwrap().vars.insert(name.clone(), val);
            }
            Stmt::Listen(_, _, _) => {
                // listen 仅顶层收集，块内忽略
            }
            Stmt::If(cond, body, else_body) => {
                let hit = {
                    let guard = rt.lock().unwrap();
                    eval_cond(cond, &guard, ctx).unwrap_or(false)
                };
                if hit {
                    box_exec(rt, body, notify, ctx).await?;
                } else if !else_body.is_empty() {
                    box_exec(rt, else_body, notify, ctx).await?;
                }
            }
            Stmt::While(body) => {
                // 单遍执行 while 体（顶层常驻循环由 run_while_tasks 驱动；
                // 事件内出现 while 只执行一遍，避免卡死事件分发）
                box_pass(rt, body, notify, ctx).await?;
            }
            Stmt::For(var, iter, body) => {
                let items: Vec<String> = {
                    let guard = rt.lock().unwrap();
                    match iter {
                    IterExpr::Control(name) => guard
                        .control(name)
                        .map(|c| c.list.clone())
                        .unwrap_or_default(),
                    IterExpr::List(l) => l.clone(),
                    IterExpr::Range(a, b) => {
                        let mut v = Vec::new();
                        let mut i = *a;
                        while i < *b {
                            v.push(i.to_string());
                            i += 1;
                        }
                        v
                    }
                    IterExpr::RangeStep(a, b, n) => {
                        let mut v = Vec::new();
                        let mut i = *a;
                        let step = n.max(&1);
                        while i < *b {
                            v.push(i.to_string());
                            i += *step;
                        }
                        v
                    }
                    }
                };
                for it in items {
                    ctx.insert(var.clone(), it);
                    box_exec(rt, body, notify, ctx).await?;
                }
                ctx.remove(var.as_str());
            }
            Stmt::Break => return Ok(()),
            Stmt::Action(action) => {
                exec_action(rt, action, notify, ctx).await?;
            }
            Stmt::Page(_, _) => {}
        }
    }
    Ok(())
}

async fn exec_action(
    rt: &Arc<Mutex<PluginRuntime>>,
    action: &Action,
    notify: &mut Vec<Notify>,
    ctx: &HashMap<String, String>,
) -> Result<(), String> {
    // 文本类动作统一在这里求值（支持 '共 ' + count + ' 个'）
    let eval = |e: &Expr| -> Result<String, String> {
        let guard = rt.lock().unwrap();
        eval_expr(e, &guard, ctx)
    };
    match action {
        Action::Copy(a, b) => {
            let (src, dst) = {
                let guard = rt.lock().unwrap();
                (guard.resolve_path(a), guard.resolve_path(b))
            };
            if !src.exists() {
                return Err(format!("复制失败：\"{}\" 不存在", a));
            }
            std::fs::create_dir_all(&dst).map_err(|e| format!("创建目标目录失败: {}", e))?;
            copy_recursive(&src, &dst)
        }
        Action::Move(a, b) => {
            let (src, dst) = {
                let guard = rt.lock().unwrap();
                (guard.resolve_path(a), guard.resolve_path(b))
            };
            if !src.exists() {
                return Err(format!("移动失败：\"{}\" 不存在", a));
            }
            std::fs::create_dir_all(&dst).map_err(|e| format!("创建目标目录失败: {}", e))?;
            let target = dst.join(src.file_name().unwrap_or(std::ffi::OsStr::new("x")));
            std::fs::rename(&src, &target).map_err(|e| format!("移动失败: {}", e))
        }
        Action::Delete(a) => {
            let p = { rt.lock().unwrap().resolve_path(a) };
            if p.exists() {
                if p.is_dir() {
                    std::fs::remove_dir_all(&p).map_err(|e| format!("删除失败: {}", e))?;
                } else {
                    std::fs::remove_file(&p).map_err(|e| format!("删除失败: {}", e))?;
                }
            }
            Ok(())
        }
        Action::Open(a) => {
            let p = { rt.lock().unwrap().resolve_path(a) };
            open::that(p.to_string_lossy().to_string()).map_err(|e| format!("打开失败: {}", e))
        }
        Action::Popup(t) => {
            notify.push(Notify { kind: "popup".to_string(), text: eval(t)? });
            Ok(())
        }
        Action::Toast(t) => {
            notify.push(Notify { kind: "toast".to_string(), text: eval(t)? });
            Ok(())
        }
        Action::Sleep(n) => {
            let n = *n;
            tokio::time::sleep(std::time::Duration::from_secs_f64(n.max(0.0))).await;
            Ok(())
        }
        Action::Mkdir(a) => {
            let p = { rt.lock().unwrap().resolve_path(a) };
            std::fs::create_dir_all(&p).map_err(|e| format!("创建目录失败: {}", e))
        }
        Action::Exists(a) => {
            let p = { rt.lock().unwrap().resolve_path(a) };
            let text = if p.exists() {
                format!("\"{}\" 存在", a)
            } else {
                format!("\"{}\" 不存在", a)
            };
            notify.push(Notify { kind: "toast".to_string(), text });
            Ok(())
        }
        Action::ListFiles(a) => {
            let p = { rt.lock().unwrap().resolve_path(a) };
            let names = match std::fs::read_dir(&p) {
                Ok(rd) => {
                    let mut v: Vec<String> = rd
                        .flatten()
                        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                        .collect();
                    v.sort();
                    v.join(", ")
                }
                Err(e) => return Err(format!("列出目录失败: {}", e)),
            };
            let text = if names.is_empty() {
                format!("\"{}\" 目录为空", a)
            } else {
                format!("\"{}\" 包含：{}", a, names)
            };
            notify.push(Notify { kind: "toast".to_string(), text });
            Ok(())
        }
        // ---- v1.1 新增 ----
        Action::WriteFile(path, text) => {
            let p = { rt.lock().unwrap().resolve_path(path) };
            let body = eval(text)?;
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }
            std::fs::write(&p, body).map_err(|e| format!("写入失败: {}", e))
        }
        Action::AppendFile(path, text) => {
            let p = { rt.lock().unwrap().resolve_path(path) };
            let body = eval(text)?;
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .map_err(|e| format!("打开文件失败: {}", e))?;
            f.write_all(body.as_bytes()).map_err(|e| format!("写入失败: {}", e))
        }
        Action::ReadFile(path) => {
            let p = { rt.lock().unwrap().resolve_path(path) };
            let body = std::fs::read_to_string(&p).map_err(|e| format!("读取失败: {}", e))?;
            // 只读前 300 个字符，避免长文件把 toast 撑爆
            let preview: String = body.chars().take(300).collect();
            let suffix = if body.chars().count() > 300 { "…" } else { "" };
            notify.push(Notify {
                kind: "toast".to_string(),
                text: format!("{}：{}{}", path, preview, suffix),
            });
            Ok(())
        }
        Action::Rename(from, to) => {
            let (src, dst) = {
                let guard = rt.lock().unwrap();
                (guard.resolve_path(from), guard.resolve_path(to))
            };
            if !src.exists() {
                return Err(format!("重命名失败：\"{}\" 不存在", from));
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }
            std::fs::rename(&src, &dst).map_err(|e| format!("重命名失败: {}", e))
        }
        Action::CopyText(t) => {
            let body = eval(t)?;
            write_clipboard(&body)
        }
        Action::Log(t) => {
            let body = eval(t)?;
            let p = { rt.lock().unwrap().plugin_dir.join("plugin.log") };
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                let _ = writeln!(f, "{}", body);
            }
            Ok(())
        }
        Action::Notify(t) => {
            notify.push(Notify { kind: "notify".to_string(), text: eval(t)? });
            Ok(())
        }
        Action::Zip(src, dst) => {
            let (s_path, d_path) = {
                let guard = rt.lock().unwrap();
                (guard.resolve_path(src), guard.resolve_path(dst))
            };
            zip_dir(&s_path, &d_path)
        }
        Action::Unzip(src, dst) => {
            let (s_path, d_path) = {
                let guard = rt.lock().unwrap();
                (guard.resolve_path(src), guard.resolve_path(dst))
            };
            unzip_into(&s_path, &d_path)
        }
        Action::LaunchGame => Err("launchGame 尚未实现（P1）".to_string()),
    }
}

/// 写系统剪贴板：三平台各用自己的命令行工具，缺工具时给出可读的错误
fn write_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else if cfg!(target_os = "windows") {
        ("cmd", &["/C", "clip"])
    } else {
        ("xclip", &["-selection", "clipboard"])
    };
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("复制到剪贴板失败（缺少 {}）：{}", cmd, e))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("写入剪贴板失败: {}", e))?;
    }
    child.wait().map_err(|e| format!("剪贴板命令失败: {}", e))?;
    Ok(())
}

/// 递归把目录/文件打包成 zip（目标已存在则覆盖）
fn zip_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("打包失败：\"{}\" 不存在", src.display()));
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let file = std::fs::File::create(dst).map_err(|e| format!("创建压缩包失败: {}", e))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::FileOptions::default();
    fn walk(
        zw: &mut zip::ZipWriter<std::fs::File>,
        root: &std::path::Path,
        cur: &std::path::Path,
        opts: zip::write::FileOptions,
    ) -> Result<(), String> {
        let rd = std::fs::read_dir(cur).map_err(|e| format!("读取目录失败: {}", e))?;
        for e in rd.flatten() {
            let p = e.path();
            let rel = p
                .strip_prefix(root)
                .map_err(|e| format!("路径计算失败: {}", e))?
                .to_string_lossy()
                .replace('\\', "/");
            if p.is_dir() {
                zw.add_directory(format!("{}/", rel), opts)
                    .map_err(|e| format!("写入目录失败: {}", e))?;
                walk(zw, root, &p, opts)?;
            } else {
                zw.start_file(rel, opts)
                    .map_err(|e| format!("写入文件失败: {}", e))?;
                let mut f = std::fs::File::open(&p).map_err(|e| format!("打开失败: {}", e))?;
                std::io::copy(&mut f, zw).map_err(|e| format!("写入失败: {}", e))?;
            }
        }
        Ok(())
    }
    if src.is_dir() {
        walk(&mut zw, src, src, opts)?;
    } else {
        let name = src
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        zw.start_file(name, opts)
            .map_err(|e| format!("写入文件失败: {}", e))?;
        let mut f = std::fs::File::open(src).map_err(|e| format!("打开失败: {}", e))?;
        std::io::copy(&mut f, &mut zw).map_err(|e| format!("写入失败: {}", e))?;
    }
    zw.finish().map_err(|e| format!("收尾失败: {}", e))?;
    Ok(())
}

/// 解压 zip 到目录（跳过含 `..` 的条目，防目录穿越）
fn unzip_into(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(src).map_err(|e| format!("打开压缩包失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析压缩包失败: {}", e))?;
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录失败: {}", e))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩包失败: {}", e))?;
        let rel = entry.mangled_name();
        if rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue;
        }
        let out = dst.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("创建目录失败: {}", e))?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }
            let mut f = std::fs::File::create(&out).map_err(|e| format!("创建文件失败: {}", e))?;
            std::io::copy(&mut entry, &mut f).map_err(|e| format!("解压失败: {}", e))?;
        }
    }
    Ok(())
}

fn copy_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if src.is_dir() {
        for e in std::fs::read_dir(src).map_err(|e| format!("读取目录失败: {}", e))? {
            let e = e.map_err(|e| format!("读取目录项失败: {}", e))?;
            let dst_sub = dst.join(e.file_name());
            copy_recursive(&e.path(), &dst_sub)?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::copy(src, dst)
            .map(|_| ())
            .map_err(|e| format!("复制文件失败 {}: {}", src.display(), e))
    }
}

// ---------------------------------------------------------------------------
// 对外 API
// ---------------------------------------------------------------------------

/// 控件树 → 前端渲染 JSON
pub fn controls_to_json(rt: &PluginRuntime) -> serde_json::Value {
    serde_json::Value::Array(
        rt.controls
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "type": c.ctype,
                    "text": c.text,
                    "layout": c.layout,
                    "list": c.list,
                    "checked": c.checked,
                    "selected": c.selected,
                    "value": c.value,
                    "lineLength": c.line_len,
                    "portrait": c.portrait,
                })
            })
            .collect(),
    )
}

pub fn page_inject_controls_to_json(rt: &PluginRuntime, page: &str) -> serde_json::Value {
    let controls = rt.page_injects.get(page);
    let overrides = rt.page_overrides.get(page);

    let ctls_json = match controls {
        Some(controls) => serde_json::Value::Array(
            controls
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "type": c.ctype,
                        "text": c.text,
                        "layout": c.layout,
                        "list": c.list,
                        "checked": c.checked,
                        "selected": c.selected,
                        "lineLength": c.line_len,
                        "portrait": c.portrait,
                    })
                })
                .collect(),
        ),
        None => serde_json::Value::Array(vec![]),
    };

    let overrides_json = match overrides {
        Some(m) => {
            let map: serde_json::Map<String, serde_json::Value> = m
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Object(serde_json::Map::new()),
    };

    json!({
        "controls": ctls_json,
        "overrides": overrides_json,
    })
}

/// 分发事件：控件事件（前端交互）或系统事件。返回 (通知列表, 控件树)。
pub async fn dispatch_event(
    rt: &Arc<Mutex<PluginRuntime>>,
    target: &ListenerTarget,
    event: &str,
    value: Option<String>,
) -> Result<(Vec<Notify>, serde_json::Value), String> {
    // 前端把输入框/滑块的当前值一并带过来，控件里的 `X.value` 才读得到
    if let (Some(v), ListenerTarget::Control(name)) = (value, target) {
        if let Some(c) = rt.lock().unwrap().control_mut(name) {
            c.value = Some(v);
        }
    }
    let handlers = {
        let guard = rt.lock().unwrap();
        guard
            .handlers
            .iter()
            .filter(|(t, e, _)| match t {
                ListenerTarget::Control(n) => {
                    matches!(target, ListenerTarget::Control(tn) if tn == n)
                }
                ListenerTarget::System(s) => {
                    matches!(target, ListenerTarget::System(ts) if ts == s)
                }
            } && (e.is_empty() || *e == event))
            .cloned()
            .collect::<Vec<_>>()
    };
    if handlers.is_empty() {
        return Ok((vec![], controls_to_json(&rt.lock().unwrap())));
    }
    let mut notify = Vec::new();
    let mut ctx = HashMap::new();
    for (_, _, body) in &handlers {
        if let Err(e) = exec_block(rt, body, &mut notify, &mut ctx).await {
            let mut guard = rt.lock().unwrap();
            let line = format!("插件执行出错: {}", e);
            let _ = &mut guard.last_error;
            guard.last_error = Some(line.clone());
            notify.push(Notify { kind: "toast".to_string(), text: line });
            drop(guard);
        }
    }
    Ok((notify, controls_to_json(&rt.lock().unwrap())))
}

/// 执行 block 内语句一遍（while 体专用；while 本身不做无限循环包裹）
async fn exec_block_pass(
    rt: &Arc<Mutex<PluginRuntime>>,
    stmts: &[Stmt],
    notify: &mut Vec<Notify>,
    ctx: &mut HashMap<String, String>,
) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            Stmt::While(body) => {
                box_pass(rt, body, notify, ctx).await?;
            }
            _ => box_exec(rt, std::slice::from_ref(stmt), notify, ctx).await?,
        }
    }
    Ok(())
}

/// 顶层 while 后台任务：每轮执行 while 体一遍 + sleep 50ms
pub async fn run_while_tasks(rt: Arc<Mutex<PluginRuntime>>) {
    let blocks = {
        let guard = rt.lock().unwrap();
        guard.while_blocks.clone()
    };
    loop {
        let mut notify = Vec::new();
        let mut ctx = HashMap::new();
        for block in &blocks {
            if let Err(e) = exec_block_pass(&rt, block, &mut notify, &mut ctx).await {
                let mut guard = rt.lock().unwrap();
                guard.last_error = Some(format!("while 执行出错: {}", e));
                drop(guard);
            }
        }
        let _ = notify;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// 触发一次顶层 if 评估——由后台任务每轮调用。
pub async fn step_loops(rt: &Arc<Mutex<PluginRuntime>>) {
    let loops = {
        let guard = rt.lock().unwrap();
        guard.loops.clone()
    };
    let mut notify = Vec::new();
    let mut ctx = HashMap::new();
    for block in &loops {
        if let Err(e) = exec_block(rt, block, &mut notify, &mut ctx).await {
            let mut guard = rt.lock().unwrap();
            guard.last_error = Some(format!("持续块执行出错: {}", e));
            drop(guard);
        }
    }
    // 持续块产生的通知也广播（P0 简化：丢弃，因为无前端监听）
    let _ = notify;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_author_example() {
        let src = r#"
name: Auto Backup
version: 1.0

Backup_Button = CreateButton('立即备份')
Backup_Button.center
Status_Text = CreateText('等待中')

listen Backup_Button.click:
    copy game-saves to backup-folder
    Status_Text = '备份完成'

listen game_quit:
    copy game-saves to backup-folder
    popup('游戏被退出了，已自动备份')
"#;
        let stmts = parse_lumi(src).expect("parse ok");
        assert!(stmts.len() >= 6);
        // 含 2 个 listen（控件 + 系统）
        let listens = stmts
            .iter()
            .filter(|s| matches!(s, Stmt::Listen(_, _, _)))
            .count();
        assert_eq!(listens, 2);
    }

    #[test]
    fn parse_single_line_forms() {
        let src = r#"
Run_Button = CreateButton('运行')
Run_Button.center
Tip_Text = CreateText('就绪')
Selector = CreateList('甲', '乙', '丙')

listen Run_Button.click(Tip_Text = '已运行')
listen Selector.changed(Tip_Text = '切换')
if Tip_Text == '就绪'(Tip_Text = '条件单行')
for 项 in 0..3:
    Tip_Text = 项
"#;
        let stmts = parse_lumi(src).expect("parse ok");
        // 4 创建 + 1 布局 + 2 listen 单行 + 1 if 单行 + 1 for 块
        assert_eq!(stmts.len(), 8, "stmts: {:?}", stmts);
    }

    #[test]
    fn run_plugin_click_flow() {
        // 构建运行时 → 触发 click → 控件文本更新
        let src = r#"
name: Demo
Msg = CreateText('等待')
listen Msg.click:
    Msg = '点过了'
    toast('你好')
"#;
        let dir = std::env::temp_dir().join("lumia-plugin-test");
        let _ = std::fs::create_dir_all(&dir);
        let rt = build_runtime(src, dir.clone()).expect("build");
        let arc = Arc::new(Mutex::new(rt));
        let rt_run = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (notify, tree) = rt_run.block_on(dispatch_event(
            &arc,
            &ListenerTarget::Control("Msg".to_string()),
            "click",
            None,
        ))
        .expect("dispatch");
        assert_eq!(notify.len(), 1, "toast: {:?}", notify);
        assert_eq!(notify[0].kind, "toast");
        assert_eq!(tree[0]["text"], "点过了");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests2 {
    use super::*;
    #[test]
    fn new_actions_parse_and_run() {
        let src = r#"
name: ActionsTest
Note = CreateText('x')
listen Note.click:
    mkdir 'tmp/new-folder'
    exists 'tmp/new-folder'
    listFiles '.'
"#;
        let dir = std::env::temp_dir().join("lumia-actions-test");
        let _ = std::fs::create_dir_all(&dir);
        let rt = build_runtime(src, dir.clone()).expect("build");
        let arc = Arc::new(Mutex::new(rt));
        let rt_run = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (notify, _tree) = rt_run.block_on(dispatch_event(
            &arc,
            &ListenerTarget::Control("Note".to_string()),
            "click",
            None,
        ))
        .expect("dispatch");
        assert_eq!(notify.len(), 2, "notify: {:?}", notify);
        assert!(notify.iter().any(|n| n.text.contains("存在")));
        assert!(dir.join("tmp/new-folder").is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// v1.1 新接口：拼接表达式 + set 变量 + 写读文件 + 压缩解压
    #[test]
    fn v11_expressions_vars_and_file_actions() {
        let src = r#"
name: V11
Note = CreateText('x')
listen Note.click:
    set count = 3
    set who = 'Crystal'
    toast('共 ' + count + ' 个 ' + who)
    writeFile 'out/note.txt' 'hello ' + who
    appendFile 'out/note.txt' '!'
    readFile 'out/note.txt'
    zip 'out' to 'out.zip'
    unzip 'out.zip' to 'unz'
"#;
        let dir = std::env::temp_dir().join(format!("lumia-v11-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let rt = build_runtime(src, dir.clone()).expect("build");
        let arc = Arc::new(Mutex::new(rt));
        let rt_run = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (notify, _tree) = rt_run
            .block_on(dispatch_event(
                &arc,
                &ListenerTarget::Control("Note".to_string()),
                "click",
                None,
            ))
            .expect("dispatch");

        // 拼接 + 变量：'共 ' + count + ' 个 ' + who
        assert!(
            notify.iter().any(|n| n.text == "共 3 个 Crystal"),
            "拼接结果不对: {:?}",
            notify.iter().map(|n| &n.text).collect::<Vec<_>>()
        );

        // writeFile + appendFile
        let body = std::fs::read_to_string(dir.join("out/note.txt")).unwrap();
        assert_eq!(body, "hello Crystal!");

        // readFile 会把内容回显到 toast
        assert!(notify.iter().any(|n| n.text.contains("hello Crystal!")));

        // zip → unzip 往返
        assert!(dir.join("out.zip").is_file(), "zip 没产出");
        assert!(dir.join("unz/note.txt").is_file(), "unzip 没解出来");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn system_event_broadcast_runs() {
        let src = r#"
name: SysTest
Note = CreateText('x')
listen game_start:
    Note = '开始'
    toast('游戏开始了')
listen game_quit:
    Note = '退出'
    popup('游戏退出了')
"#;
        let dir = std::env::temp_dir().join("lumia-sys-test");
        let _ = std::fs::create_dir_all(&dir);
        let rt = build_runtime(src, dir.clone()).expect("build");
        let arc = Arc::new(Mutex::new(rt));
        let rt_run = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (notify, tree) = rt_run.block_on(dispatch_event(
            &arc,
            &ListenerTarget::System("game_start".to_string()),
            "",
            None,
        ))
        .expect("dispatch");
        assert!(notify.iter().any(|n| n.kind == "toast" && n.text.contains("游戏开始了")), "no toast, got {:?}", notify);
        assert_eq!(tree[0]["text"], "开始");
        let (notify2, tree2) = rt_run.block_on(dispatch_event(
            &arc,
            &ListenerTarget::System("game_quit".to_string()),
            "",
            None,
        ))
        .expect("dispatch");
        assert!(notify2.iter().any(|n| n.kind == "popup" && n.text.contains("游戏退出了")));
        assert_eq!(tree2[0]["text"], "退出");
        let _ = std::fs::remove_dir_all(&dir);
    }
}