<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center"><a href="README.md">🇺🇸 English</a> | <a href="README_spa.md">🇪🇸 Español</a> | <a href="README_fra.md">🇫🇷 Français</a> | <a href="README_ita.md">🇮🇹 Italiano</a> | <a href="README_deu.md">🇩🇪 Deutsch</a> | <a href="README_zho.md">🇨🇳 中文</a> | <a href="README_jpn.md">🇯🇵 日本語</a> | 🇵🇹 <b>Português</b></p>

### EH3D-PHOENIX-CAD: planeado como um CAD mecânico paramétrico centrado no projeto de caixas, chassis e dissipadores térmicos. Existe um primeiro modelo de caixas como biblioteca com uma janela que nunca foi aberta num ecrã; nada foi executado numa máquina.

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Rust%20%2B%20Markdown%20%2B%20Python-blue.svg" alt="Rust, Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-564-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.0.4-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**Verificação de honestidade - o que funciona hoje:** uma biblioteca Rust com uma caixa paramétrica: comprimentos em nanómetros inteiros com o seu próprio intervalo, furos nas faces (redondos, escareados, com rebaixo para a cabeça, retangulares e ovais), arestas arredondadas e chanfradas, parâmetros escritos como fórmulas com unidades sobre variáveis, constantes e outros parâmetros (frações exatas, excesso verificado), operações de alteração estruturadas que comunicam os conflitos e deixam o modelo intacto, postes sob os furos de fixação de uma placa com uma tampa aparafusada neles, a receita da caixa guardada no domínio mecânico do ficheiro de projeto, um script ao estilo OpenSCAD e uma verificação de encaixe contra o envelope de uma placa. Uma janela (egui, com `--features gui`) mostra a árvore do modelo, as propriedades de cada elemento, uma vista 3D e uma planta (onde se arrastam furos e postes com o rato e se medem distâncias e ângulos; o script, o ficheiro STEP e o relatório de ajuste são escritos num ficheiro a partir de um menu, nunca sobre um ficheiro que existe, a menos que a caixa para isso esteja assinalada), e um comando `check` lê um projeto e imprime as suas medidas, o encaixe e o estado de cada passo; a lógica da janela é testada sem ecrã. Tem 561 testes próprios e passam num computador. Só foi experimentada com modelos feitos pelos seus próprios testes; a janela nunca foi aberta num ecrã e nada foi executado numa máquina, e o ficheiro STEP que escreve só foi verificado na sua estrutura, e o esboço 2D (geometria, restrições, um solucionador e uma janela para o desenhar e cotar), que é guardado no ficheiro de projeto e cujos contornos fechados podem ser extrudidos a partir de uma face ou escavados nela como passos da receita, só foi experimentado com esboços feitos pelos seus próprios testes e a sua janela ainda não foi aberta num ecrã, enquanto os padrões de furos e postes e as alhetas de arrefecimento (com uma estimativa simples do calor que cedem ao ar parado, comparada com nenhuma medição) existem mas só foram experimentados com modelos feitos pelos seus próprios testes, e as montagens estão apenas planeadas.

---

> **Estado de segurança (safety) e de segurança informática:** este repositório está nos primeiros passos e não é um sistema de segurança (safety). Nada do que está escrito aqui substitui os limites, os bloqueios e a paragem de emergência da máquina e do seu controlador.

## 📋 Num relance

| | |
|---|---|
| Versão | `0.0.4` |
| Maturidade | Estrutura inicial |
| Testes | 564 |
| Tecnologias | Rust, Markdown, Python 3.10+ |
| Em hardware real | Ainda não executado em hardware |

**Ir para:** [Visão geral](#-visão-geral) · [Arquitetura](#-arquitetura) · [Início rápido](#-início-rápido) · [Testes](#-testes) · [Estrutura do repositório](#-estrutura-do-repositório) · [Projetos relacionados](#-projetos-relacionados)

## 🚦 Estado do projeto

Cada linha é uma parte deste repositório e a luz diz até onde chega hoje. É escrita com o que o repositório consegue demonstrar (testes, compilações, execuções), não com o que está previsto.

🟢 Feito e verificado · 🟡 Funciona, com limites conhecidos · 🟠 Feito a meio ou ainda por verificar · 🔴 Por fazer ou a não funcionar

| Área | Estado | O que significa aqui |
|---|---|---|
| Verificações do repositório | 🟡 | A verificação do repositório, a das imagens e os 3 testes da base correm na CI e passam. |
| Testes | 🟡 | Os 561 testes da biblioteca Rust e os 3 da base passam num computador. Usam apenas modelos feitos pelos próprios testes. |
| Hardware real | 🟠 | Não foi executado em nenhuma máquina nem placa: nada aqui foi experimentado em hardware. |
| Função planeada | 🟠 | Feita em parte: existem uma caixa paramétrica com furos, postes e uma tampa aparafusada, um script ao estilo OpenSCAD, uma verificação de encaixe e a receita da caixa no ficheiro de projeto, e um primeiro escritor de STEP verificado só na estrutura; existe um esboço 2D com restrições, um solucionador e uma janela para o desenhar, guardado no ficheiro de projeto e que pode ser extrudido a partir de uma face ou escavado nela, mas a sua janela ainda não foi aberta num ecrã; existem padrões de furos e postes e alhetas de arrefecimento com uma estimativa do seu calor, experimentados só com modelos feitos pelos testes; as montagens não foram começadas. |
| Requisitos | 🟠 | 5 de 6 requisitos estão em protótipo (a caixa, o script, as operações de alteração, o modelo de dissipador e o intervalo mecânico), 1 está apenas planeado e nenhum está validado. |

## 🎯 Visão geral

* **Planeado:** O EH3D-PHOENIX-CAD pretende ser um CAD mecânico paramétrico centrado no projeto de caixas, chassis e dissipadores, como biblioteca com janela, com interface de linha de comandos e, quando se aplicar, um tipo de tarefa do servidor. A biblioteca existe com uma primeira caixa paramétrica, uma janela que mostra a árvore, as propriedades, uma vista 3D e uma planta, onde se arrastam furos e postes com o rato e se medem distâncias e ângulos (nunca aberta num ecrã) e um comando `check`; a tarefa do servidor ainda não existe, as alhetas de arrefecimento (com uma estimativa do seu calor) não estão no ficheiro STEP, e o ficheiro STEP só foi verificado na sua estrutura.

## 🧩 Papel no EH3D

EH3D-PHOENIX-CAD faz parte da família PHOENIX da EH3D Fabrication Suite. O que fará, o que ainda não fará, os seus requisitos e as suas relações com outros repositórios estão escritos na pasta `docs/`.

## 🏛️ Arquitetura

<p align="center">
  <img src="images/architecture.svg" alt="Arquitetura de EH3D-PHOENIX-CAD: quem o usa, o que contém e de que depende" width="100%">
</p>

Como as partes deste repositório se encaixam: quem o usa, o que contém e de que depende. A imagem é desenhada a partir de `docs/architecture.json` e verificada com o código.

## 🚀 Início rápido

Precisa de Python 3.10 ou mais recente; não há nada a instalar.

```bash
python -I tools/ci_validate.py
python -I tools/make_architecture.py --check
python -I tools/make_structure.py --check --readmes
python -I -m unittest discover -s tools -p "test_*.py"
```

Para experimentar o programa (precisa de Rust; a janela precisa de `--features gui`):

```bash
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/plate.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixproj --shot box.png
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixsketch --view sketch --shot sketch.png
```

## 🧪 Testes

A biblioteca Rust tem 561 testes (`cargo test`) e a base tem 3 (`python -I -m unittest discover -s tools -p "test_*.py"`); todos passam num computador. A CI executa a verificação do repositório, a das imagens, os testes da base e, para o código Rust, a verificação de formato, o clippy com avisos como erros e os testes em Linux e Windows, e verifica também a janela com o clippy (`--features gui`).

## 🏗️ Compilação e publicação

A versão vive no ficheiro `VERSION` e em `eh3d.project.json`, e cada versão tem a sua entrada em [CHANGELOG.md](CHANGELOG.md). O programa compila-se com `cargo build --release` (a janela com `--features gui`); as versões são publicadas no GitHub.

## 📂 Estrutura do repositório

<p align="center">
  <img src="images/repository-structure.svg" alt="Estrutura do repositório EH3D-PHOENIX-CAD" width="100%">
</p>

<details>
<summary>Versão em texto da árvore</summary>

```text
EH3D-PHOENIX-CAD/
├── docs/
│   ├── architecture.json structure.json         os dados das duas imagens
│   ├── SCOPE.md REQUIREMENTS.md                 o que está planeado e os seus requisitos
│   ├── DEPENDENCIES.md THREAT_MODEL.md          relações com outros repositórios e modelo de ameaças
│   ├── DECISIONS.md STATUS.md                   decisões que se aplicam aqui e estado honesto
│   ├── NOT_YET.md RULES.md                      ideias para mais tarde e regras para escrever código
│   ├── ERROR_CODES.md                           os códigos de diagnóstico PHOENIX-<ÁREA>-<NNN>
│   ├── EXPRESSIONS.md                           a linguagem das fórmulas dos parâmetros
│   ├── PROJECT_DOMAIN.md                        a receita da caixa no ficheiro de projeto
│   ├── SKETCH.md                                o esboço 2D: restrições, solucionador, ficheiro de projeto e extrusões
│   └── adr/                                     os registos das decisões aceites
├── Cargo.toml Cargo.lock                        o espaço de trabalho Rust e as suas dependências fixadas
├── crates/                                      modelo mecânico: caixa, padrões, alhetas, esboços, script, verificação de encaixe, janela
├── AGENTS.md                                    regras para quem altera o repositório
├── tools/                                       a verificação do repositório, os seus testes e as ferramentas das imagens
├── images/                                      banner e as imagens de Arquitetura e Estrutura do repositório
├── .github/                                     fluxos de CI e do painel, modelos de issues e pull requests
├── eh3d.project.json                            manifesto: versão, papel, maturidade e comando de compilação
├── VERSION                                      a versão do repositório
├── CHANGELOG.md                                 o que mudou em cada versão
├── SECURITY.md SUPPORT.md                       como comunicar uma vulnerabilidade e onde perguntar
└── CONTRIBUTING.md CODE_OF_CONDUCT.md LICENSE   como contribuir, o código de conduta e a licença
```

</details>

## 🗺️ Roteiro e limites conhecidos

**Limites conhecidos hoje**

* Código inicial: o modelo de uma caixa com os seus esboços, extrusões e cavidades, experimentado com modelos feitos pelos seus próprios testes; nada foi executado numa máquina e nenhum requisito está validado.

**A seguir**

* Fazer a ferramenta de placas ler e escrever o domínio mecânico do formato de projeto, abrir o ficheiro STEP num programa de CAD e depois acrescentar as montagens e comparar a estimativa das alhetas de arrefecimento com uma medição, cada um com os seus testes.

## 🌍 Idiomas

O README e os textos existem em inglês, espanhol, francês, italiano, alemão, chinês, japonês e português europeu.

## 🔗 Projetos relacionados

* [EH3D-DOCS](https://github.com/JuanenRac/EH3D-DOCS): o mapa da suite, a sua arquitetura, as convenções e as verificações partilhadas
* [EH3D-COMMON](https://github.com/JuanenRac/EH3D-COMMON): a identidade de objeto (UUID v7) que os furos usam; mais tarde o contentor de projeto e a crate de diagnóstico
* Relações planeadas, ainda por construir (EH3D-PHOENIX-PCB, EH3D-THERMAL-LAB): ver [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)

## 📚 Documentação

* [As convenções da suite](https://github.com/JuanenRac/EH3D-DOCS/blob/main/docs/CONVENTIONS.md)
* [Registo de alterações](CHANGELOG.md)
* [Âmbito, requisitos, dependências, modelo de ameaças e decisões](docs/SCOPE.md)

## 🤝 Contribuir e comunidade

* Antes de um pull request, execute `python -I tools/ci_validate.py` e os testes da base: tudo tem de passar. Consulte [CONTRIBUTING.md](CONTRIBUTING.md) e o [código de conduta](CODE_OF_CONDUCT.md).
* O README existe em oito idiomas: uma alteração ao texto faz-se nos oito ficheiros, e as duas imagens são desenhadas a partir de `docs/architecture.json` e `docs/structure.json` com `tools/make_architecture.py` e `tools/make_structure.py`.
* Comunique uma vulnerabilidade em privado: [SECURITY.md](SECURITY.md). Perguntas, ideias e relatórios: electrohobby3d@gmail.com ([SUPPORT.md](SUPPORT.md)).

## 👤 AUTOR

**JuanenRac (Electro Hobby 3D)** · electrohobby3d@gmail.com

## 📜 LICENÇA

Software: GPL-3.0-or-later. Ver [LICENSE](LICENSE). Os projetos de hardware da suite usam a CERN OHL-S.
