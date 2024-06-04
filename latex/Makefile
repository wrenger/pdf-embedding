TEX = latexmk -halt-on-error -file-line-error -synctex=1 --shell-escape -bibtex -outdir=build -f

all: main.pdf
.PHONY: all clean main.pdf

main.pdf: main.tex
	@mkdir -p build
	$(TEX) -pdf $<
	@cp -f build/$(notdir $@) $@

clean:
	$(TEX) -c
	$(TEX) -CA
	@rm -rf build main.pdf
