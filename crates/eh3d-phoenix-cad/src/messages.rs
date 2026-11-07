//! The text of every diagnostic in the eight languages of the suite.
//!
//! A program shows the text of `diag.<code>` in the language of the person; the code itself
//! (`PHOENIX-CAD-NNN`) is never translated. A test checks that every code of the registry has a
//! text in every language and that the English text is the title of the registry.

/// The languages, in the order of the texts: English, Spanish, German, French, Italian, Japanese,
/// Chinese and European Portuguese.
pub const LANGUAGES: [&str; 8] = ["en", "es", "de", "fr", "it", "ja", "zh", "pt"];

type Row = (&'static str, [&'static str; 8]);

/// The texts by code.
const TEXTS: &[Row] = &[
    (
        "PHOENIX-CAD-001",
        [
            "Length outside the mechanical range",
            "Longitud fuera del rango mecánico",
            "Länge außerhalb des mechanischen Bereichs",
            "Longueur hors de la plage mécanique",
            "Lunghezza fuori dall'intervallo meccanico",
            "長さが機構の範囲外です",
            "长度超出机械范围",
            "Comprimento fora do intervalo mecânico",
        ],
    ),
    (
        "PHOENIX-CAD-002",
        [
            "Parameter is not positive",
            "El parámetro no es positivo",
            "Der Parameter ist nicht positiv",
            "Le paramètre n'est pas positif",
            "Il parametro non è positivo",
            "パラメータが正の値ではありません",
            "参数不是正数",
            "O parâmetro não é positivo",
        ],
    ),
    (
        "PHOENIX-CAD-003",
        [
            "Hole outside its face",
            "Agujero fuera de su cara",
            "Bohrung außerhalb ihrer Fläche",
            "Trou en dehors de sa face",
            "Foro fuori dalla sua faccia",
            "穴が面の外にあります",
            "孔位于其面之外",
            "Furo fora da sua face",
        ],
    ),
    (
        "PHOENIX-CAD-004",
        [
            "Holes too close",
            "Agujeros demasiado cerca",
            "Bohrungen zu nah beieinander",
            "Trous trop proches",
            "Fori troppo vicini",
            "穴が近すぎます",
            "孔距过近",
            "Furos demasiado próximos",
        ],
    ),
    (
        "PHOENIX-CAD-005",
        [
            "Hole or post id used twice or unknown",
            "Identificador de agujero o de poste repetido o desconocido",
            "Bohrungs- oder Pfosten-ID doppelt verwendet oder unbekannt",
            "Identifiant de trou ou de colonnette en double ou inconnu",
            "Identificatore di foro o di colonnina usato due volte o sconosciuto",
            "穴または支柱の ID が重複しているか不明です",
            "孔或立柱的标识重复或未知",
            "Identificador de furo ou de poste repetido ou desconhecido",
        ],
    ),
    (
        "PHOENIX-CAD-006",
        [
            "Angle outside the range",
            "Ángulo fuera de rango",
            "Winkel außerhalb des Bereichs",
            "Angle hors de la plage",
            "Angolo fuori intervallo",
            "角度が範囲外です",
            "角度超出范围",
            "Ângulo fora do intervalo",
        ],
    ),
    (
        "PHOENIX-CAD-007",
        [
            "Count or ratio outside its range",
            "Cantidad o proporción fuera de rango",
            "Anzahl oder Verhältnis außerhalb des Bereichs",
            "Nombre ou rapport hors de la plage",
            "Conteggio o rapporto fuori intervallo",
            "個数または比率が範囲外です",
            "数量或比例超出范围",
            "Contagem ou razão fora do intervalo",
        ],
    ),
    (
        "PHOENIX-CAD-008",
        [
            "Number with a unit not understood",
            "No se entiende el número con su unidad",
            "Zahl mit Einheit nicht verstanden",
            "Nombre avec unité non compris",
            "Numero con unità non compreso",
            "単位付きの数値を解釈できません",
            "无法理解带单位的数字",
            "Número com unidade não compreendido",
        ],
    ),
    (
        "PHOENIX-CAD-009",
        [
            "Integer calculation overflows",
            "El cálculo entero se desborda",
            "Ganzzahlberechnung läuft über",
            "Le calcul entier déborde",
            "Il calcolo intero va in overflow",
            "整数計算がオーバーフローします",
            "整数计算溢出",
            "O cálculo inteiro excede o limite",
        ],
    ),
    (
        "PHOENIX-CAD-010",
        [
            "Reference to an element that does not exist",
            "Referencia a un elemento que no existe",
            "Verweis auf ein nicht vorhandenes Element",
            "Référence à un élément inexistant",
            "Riferimento a un elemento inesistente",
            "存在しない要素への参照です",
            "引用了不存在的元素",
            "Referência a um elemento que não existe",
        ],
    ),
    (
        "PHOENIX-CAD-011",
        [
            "Settings not valid",
            "Ajustes no válidos",
            "Einstellungen ungültig",
            "Réglages non valides",
            "Impostazioni non valide",
            "設定が無効です",
            "设置无效",
            "Definições inválidas",
        ],
    ),
    (
        "PHOENIX-CAD-012",
        [
            "File cannot be read or written",
            "No se puede leer o escribir el archivo",
            "Datei kann nicht gelesen oder geschrieben werden",
            "Le fichier ne peut pas être lu ou écrit",
            "Impossibile leggere o scrivere il file",
            "ファイルを読み書きできません",
            "无法读取或写入文件",
            "Não é possível ler ou escrever o ficheiro",
        ],
    ),
    (
        "PHOENIX-CAD-013",
        [
            "Dependency cycle",
            "Ciclo de dependencias",
            "Abhängigkeitszyklus",
            "Cycle de dépendances",
            "Ciclo di dipendenze",
            "依存関係の循環です",
            "依赖循环",
            "Ciclo de dependências",
        ],
    ),
    (
        "PHOENIX-CAD-014",
        [
            "Unknown name or id",
            "Nombre o identificador desconocido",
            "Unbekannter Name oder unbekannte ID",
            "Nom ou identifiant inconnu",
            "Nome o identificatore sconosciuto",
            "不明な名前または ID です",
            "未知的名称或标识",
            "Nome ou identificador desconhecido",
        ],
    ),
    (
        "PHOENIX-CAD-015",
        [
            "Name or id already used",
            "Nombre o identificador ya usado",
            "Name oder ID bereits vergeben",
            "Nom ou identifiant déjà utilisé",
            "Nome o identificatore già in uso",
            "名前または ID は既に使われています",
            "名称或标识已被使用",
            "Nome ou identificador já utilizado",
        ],
    ),
    (
        "PHOENIX-CAD-016",
        [
            "Object still in use",
            "El objeto todavía se usa",
            "Objekt wird noch verwendet",
            "Objet encore utilisé",
            "Oggetto ancora in uso",
            "オブジェクトはまだ使用されています",
            "对象仍在使用中",
            "O objeto ainda está em uso",
        ],
    ),
    (
        "PHOENIX-CAD-017",
        [
            "Name not valid",
            "Nombre no válido",
            "Name ungültig",
            "Nom non valide",
            "Nome non valido",
            "名前が無効です",
            "名称无效",
            "Nome inválido",
        ],
    ),
    (
        "PHOENIX-CAD-018",
        [
            "Project file not valid",
            "Archivo de proyecto no válido",
            "Projektdatei ungültig",
            "Fichier de projet non valide",
            "File di progetto non valido",
            "プロジェクトファイルが無効です",
            "项目文件无效",
            "Ficheiro de projeto inválido",
        ],
    ),
    (
        "PHOENIX-CAD-019",
        [
            "Edge treatment not valid",
            "Tratamiento de arista no válido",
            "Kantenbehandlung ungültig",
            "Traitement d'arête non valide",
            "Trattamento dello spigolo non valido",
            "エッジ処理が無効です",
            "边缘处理无效",
            "Tratamento de aresta inválido",
        ],
    ),
    (
        "PHOENIX-CAD-020",
        [
            "Hole shape not valid",
            "Forma de agujero no válida",
            "Bohrungsform ungültig",
            "Forme de trou non valide",
            "Forma del foro non valida",
            "穴の形状が無効です",
            "孔的形状无效",
            "Forma de furo inválida",
        ],
    ),
    (
        "PHOENIX-CAD-021",
        [
            "Screw size not in the table",
            "Tamaño de tornillo que no está en la tabla",
            "Schraubengröße nicht in der Tabelle",
            "Taille de vis absente du tableau",
            "Misura di vite non presente nella tabella",
            "ねじのサイズが表にありません",
            "螺钉规格不在表中",
            "Tamanho de parafuso que não está na tabela",
        ],
    ),
    (
        "PHOENIX-CAD-022",
        [
            "Shape not drawn by this export",
            "Forma que esta exportación no dibuja",
            "Form, die dieser Export nicht zeichnet",
            "Forme que cet export ne dessine pas",
            "Forma che questa esportazione non disegna",
            "この書き出しでは描画されない形状です",
            "此导出不绘制的形状",
            "Forma que esta exportação não desenha",
        ],
    ),
    (
        "PHOENIX-CAD-023",
        [
            "Expression not valid",
            "Expresión no válida",
            "Ausdruck ungültig",
            "Expression non valide",
            "Espressione non valida",
            "式が無効です",
            "表达式无效",
            "Expressão inválida",
        ],
    ),
    (
        "PHOENIX-CAD-024",
        [
            "Expression cannot be evaluated",
            "No se puede evaluar la expresión",
            "Ausdruck kann nicht ausgewertet werden",
            "L'expression ne peut pas être évaluée",
            "Impossibile valutare l'espressione",
            "式を評価できません",
            "无法计算表达式",
            "Não é possível avaliar a expressão",
        ],
    ),
    (
        "PHOENIX-CAD-025",
        [
            "Expression limit exceeded",
            "Se supera un límite de la expresión",
            "Grenze des Ausdrucks überschritten",
            "Limite de l'expression dépassée",
            "Limite dell'espressione superato",
            "式の上限を超えました",
            "超出表达式限制",
            "Limite da expressão excedido",
        ],
    ),
    (
        "PHOENIX-CAD-026",
        [
            "Expression arithmetic overflows",
            "La aritmética de la expresión se desborda",
            "Die Arithmetik des Ausdrucks läuft über",
            "L'arithmétique de l'expression déborde",
            "L'aritmetica dell'espressione va in overflow",
            "式の演算がオーバーフローします",
            "表达式运算溢出",
            "A aritmética da expressão excede o limite",
        ],
    ),
    (
        "PHOENIX-CAD-027",
        [
            "Post shape not valid",
            "Forma de poste no válida",
            "Pfostenform ungültig",
            "Forme de colonnette non valide",
            "Forma della colonnina non valida",
            "支柱の形状が無効です",
            "立柱的形状无效",
            "Forma de poste inválida",
        ],
    ),
    (
        "PHOENIX-CAD-028",
        [
            "Post meets a wall, a post or a hole",
            "El poste choca con una pared, otro poste o un agujero",
            "Pfosten stößt an eine Wand, einen Pfosten oder eine Bohrung",
            "La colonnette touche une paroi, une colonnette ou un trou",
            "La colonnina urta una parete, una colonnina o un foro",
            "支柱が壁、別の支柱、または穴に干渉しています",
            "立柱与墙壁、其他立柱或孔发生冲突",
            "O poste choca com uma parede, outro poste ou um furo",
        ],
    ),
    (
        "PHOENIX-CAD-029",
        [
            "Post too high for the space above it",
            "Poste demasiado alto para el espacio que tiene encima",
            "Pfosten zu hoch für den Raum darüber",
            "Colonnette trop haute pour l'espace au-dessus",
            "Colonnina troppo alta per lo spazio sopra di essa",
            "支柱が上の空間に対して高すぎます",
            "立柱对于上方空间过高",
            "Poste demasiado alto para o espaço acima dele",
        ],
    ),
    (
        "PHOENIX-CAD-030",
        [
            "Lid fixing not valid",
            "Fijación de la tapa no válida",
            "Deckelbefestigung ungültig",
            "Fixation du couvercle non valide",
            "Fissaggio del coperchio non valido",
            "蓋の固定が無効です",
            "盖子的固定方式无效",
            "Fixação da tampa inválida",
        ],
    ),
    (
        "PHOENIX-CAD-031",
        [
            "Sketch element not valid",
            "Elemento del croquis no válido",
            "Skizzenelement ungültig",
            "Élément de l'esquisse non valide",
            "Elemento dello schizzo non valido",
            "スケッチ要素が無効です",
            "草图元素无效",
            "Elemento do esboço inválido",
        ],
    ),
    (
        "PHOENIX-CAD-032",
        [
            "Sketch constraint not valid",
            "Restricción del croquis no válida",
            "Skizzenbedingung ungültig",
            "Contrainte de l'esquisse non valide",
            "Vincolo dello schizzo non valido",
            "スケッチ拘束が無効です",
            "草图约束无效",
            "Restrição do esboço inválida",
        ],
    ),
    (
        "PHOENIX-CAD-033",
        [
            "Sketch over-constrained",
            "Croquis sobrerrestringido",
            "Skizze überbestimmt",
            "Esquisse sur-contrainte",
            "Schizzo sovravincolato",
            "スケッチが過拘束です",
            "草图过约束",
            "Esboço sobrerrestringido",
        ],
    ),
    (
        "PHOENIX-CAD-034",
        [
            "Sketch solver did not converge",
            "El solucionador del croquis no converge",
            "Der Skizzenlöser konvergiert nicht",
            "Le solveur de l'esquisse ne converge pas",
            "Il risolutore dello schizzo non converge",
            "スケッチのソルバーが収束しません",
            "草图求解器未收敛",
            "O solucionador do esboço não converge",
        ],
    ),
    (
        "PHOENIX-CAD-035",
        [
            "Sketch file not valid",
            "Fichero del croquis no válido",
            "Skizzendatei ungültig",
            "Fichier de l'esquisse non valide",
            "File dello schizzo non valido",
            "スケッチファイルが無効です",
            "草图文件无效",
            "Ficheiro do esboço inválido",
        ],
    ),
    (
        "PHOENIX-CAD-036",
        [
            "Profile of the sketch not valid",
            "Perfil del croquis no válido",
            "Profil der Skizze ungültig",
            "Profil de l'esquisse non valide",
            "Profilo dello schizzo non valido",
            "スケッチのプロファイルが無効です",
            "草图轮廓无效",
            "Perfil do esboço inválido",
        ],
    ),
    (
        "PHOENIX-CAD-037",
        [
            "Extrusion or pocket not valid",
            "Extrusión o vaciado no válido",
            "Extrusion oder Tasche ungültig",
            "Extrusion ou poche non valide",
            "Estrusione o tasca non valida",
            "押し出しまたはポケットが無効です",
            "拉伸或切除无效",
            "Extrusão ou cavidade inválida",
        ],
    ),
    (
        "PHOENIX-CAD-038",
        [
            "Measurement not valid",
            "Medición no válida",
            "Messung ungültig",
            "Mesure non valide",
            "Misura non valida",
            "測定が無効です",
            "测量无效",
            "Medição inválida",
        ],
    ),
    (
        "PHOENIX-CAD-039",
        [
            "Drag not possible",
            "Arrastre no posible",
            "Ziehen nicht möglich",
            "Déplacement impossible",
            "Trascinamento non possibile",
            "ドラッグできません",
            "无法拖动",
            "Arrastamento impossível",
        ],
    ),
    (
        "PHOENIX-CAD-040",
        [
            "File exists and was not written over",
            "El fichero existe y no se sobrescribió",
            "Datei vorhanden und nicht überschrieben",
            "Le fichier existe et n'a pas été écrasé",
            "Il file esiste e non è stato sovrascritto",
            "ファイルが存在するため上書きしませんでした",
            "文件已存在，未覆盖",
            "O ficheiro existe e não foi substituído",
        ],
    ),
    (
        "PHOENIX-CAD-041",
        [
            "Export not possible",
            "Exportación no posible",
            "Export nicht möglich",
            "Exportation impossible",
            "Esportazione non possibile",
            "エクスポートできません",
            "无法导出",
            "Exportação impossível",
        ],
    ),
    (
        "PHOENIX-CAD-042",
        [
            "Pattern not valid",
            "Patrón no válido",
            "Muster ungültig",
            "Motif non valide",
            "Schema non valido",
            "パターンが無効です",
            "阵列无效",
            "Padrão inválido",
        ],
    ),
    (
        "PHOENIX-CAD-043",
        [
            "Cooling fins not valid",
            "Aletas de refrigeración no válidas",
            "Kühlrippen ungültig",
            "Ailettes de refroidissement non valides",
            "Alette di raffreddamento non valide",
            "冷却フィンが無効です",
            "散热片无效",
            "Alhetas de arrefecimento inválidas",
        ],
    ),
    (
        "PHOENIX-CAD-044",
        [
            "Library of parts not valid",
            "Biblioteca de piezas no válida",
            "Teilebibliothek ungültig",
            "Bibliothèque de pièces non valide",
            "Libreria di parti non valida",
            "部品ライブラリが無効です",
            "零件库无效",
            "Biblioteca de peças inválida",
        ],
    ),
    (
        "PHOENIX-CAD-045",
        [
            "Part not valid",
            "Pieza no válida",
            "Teil ungültig",
            "Pièce non valide",
            "Parte non valida",
            "部品が無効です",
            "零件无效",
            "Peça inválida",
        ],
    ),
    (
        "PHOENIX-CAD-046",
        [
            "Assembly not valid",
            "Ensamblaje no válido",
            "Baugruppe ungültig",
            "Assemblage non valide",
            "Assieme non valido",
            "アセンブリが無効です",
            "装配无效",
            "Montagem inválida",
        ],
    ),
    (
        "PHOENIX-CAD-047",
        [
            "Parts interfere",
            "Las piezas interfieren",
            "Teile durchdringen sich",
            "Les pièces s'interpénètrent",
            "Le parti si interferiscono",
            "部品が干渉しています",
            "零件相互干涉",
            "As peças interferem",
        ],
    ),
];

/// The text of a diagnostic in a language (`en`, `es`, `de`, `fr`, `it`, `ja`, `zh` or `pt`), or
/// `None` when the code or the language is unknown.
pub fn message(code: &str, language: &str) -> Option<&'static str> {
    let column = LANGUAGES.iter().position(|l| *l == language)?;
    TEXTS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, texts)| texts[column])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::REGISTRY;

    #[test]
    fn every_code_has_a_text_in_every_language_and_nothing_else_does() {
        assert_eq!(TEXTS.len(), REGISTRY.len());
        for (code, title) in REGISTRY {
            let row = TEXTS
                .iter()
                .find(|(c, _)| c == code)
                .unwrap_or_else(|| panic!("{code} has no text"));
            assert_eq!(row.1[0], *title, "{code}: English is the registry title");
            for (i, text) in row.1.iter().enumerate() {
                assert!(!text.trim().is_empty(), "{code} {}", LANGUAGES[i]);
            }
        }
        for (code, _) in TEXTS {
            assert!(REGISTRY.iter().any(|(c, _)| c == code), "{code}");
        }
    }

    #[test]
    fn each_translation_differs_from_the_english_and_the_asian_ones_are_not_ascii() {
        for (code, texts) in TEXTS {
            for (i, text) in texts.iter().enumerate().skip(1) {
                assert_ne!(*text, texts[0], "{code} {}", LANGUAGES[i]);
            }
            for i in [5, 6] {
                assert!(!texts[i].is_ascii(), "{code} {}", LANGUAGES[i]);
            }
        }
    }

    #[test]
    fn the_text_is_found_by_code_and_language() {
        assert_eq!(
            message("PHOENIX-CAD-004", "de"),
            Some("Bohrungen zu nah beieinander")
        );
        assert_eq!(message("PHOENIX-CAD-004", "en"), Some("Holes too close"));
        assert_eq!(message("PHOENIX-CAD-004", "xx"), None);
        assert_eq!(message("PHOENIX-CAD-999", "en"), None);
    }

    #[test]
    fn the_portuguese_is_the_one_of_portugal() {
        // the words of Brazil would be arquivo, configurações and tela
        let all: String = TEXTS.iter().flat_map(|(_, t)| t[7].chars()).collect();
        assert!(!all.contains("arquivo"));
        assert!(TEXTS.iter().any(|(_, t)| t[7].contains("ficheiro")));
        assert!(TEXTS.iter().any(|(_, t)| t[7].contains("Definições")));
    }
}
