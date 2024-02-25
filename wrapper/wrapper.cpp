#include "wrapper.hpp"

#include <istream>
#include <sstream>
#include <iostream>
#include <string.h>


class membuf : public std::basic_streambuf<char>
{
public:
    membuf(const uint8_t *p, size_t l)
    {
        setg((char *)p, (char *)p, (char *)p + l);
    }
};

class memstream : public std::istream
{
public:
    memstream(const uint8_t *p, size_t l) : std::istream(&_buffer),
                                            _buffer(p, l)
    {
        rdbuf(&_buffer);
    }

private:
    membuf _buffer;
};

extern "C" int cg3_rs_init()
{
    if (!cg3_init(stdin, stdout, stderr))
    {
        return 0;
    }
    return 1;
}

extern "C" const cg3_applicator *cg3_applicator_new(
    const uint8_t *grammar_data,
    size_t grammar_size,
    cg3_grammar *grammar_ptr)
{
    grammar_ptr = cg3_grammar_load_buffer((const char *)grammar_data, grammar_size);

    if (!grammar_ptr)
    {
        return nullptr;
    }

    auto applicator = cg3_applicator_create(grammar_ptr);
    if (!applicator)
    {
        return nullptr;
    }

    return applicator;
}

extern "C" void cg3_applicator_delete(
    cg3_applicator *ptr,
    cg3_grammar *grammar_ptr)
{
    cg3_applicator_free(ptr);
    cg3_grammar_free(grammar_ptr);
}

extern "C" const char *cg3_applicator_run(
    cg3_applicator *applicator,
    const uint8_t *input_data,
    size_t input_size,
    size_t *output_size)
{
    memstream input_stream(input_data, input_size);
    auto output = new std::stringstream(std::ios::in | std::ios::out | std::ios::binary);

    cg3_run_grammar_on_text(applicator, &input_stream, output);

    output->seekg(0, output->end);
    *output_size = output->tellg();

    return strdup(output->str().c_str());
}

extern "C" void cg3_free(void *ptr)
{
    free(ptr);
}

extern "C" cg3_mwesplitapplicator *
cg3_mwesplit_new()
{
    return cg3_mwesplitapplicator_create();
}

extern "C" const char *
cg3_mwesplit_run(
    cg3_mwesplitapplicator *applicator,
    const uint8_t *input_data,
    size_t input_size,
    size_t *output_size)
{
    memstream input_stream(input_data, input_size);
    auto output = new std::stringstream(std::ios::in | std::ios::out | std::ios::binary);

    cg3_run_grammar_on_text(applicator, &input_stream, output);

    output->seekg(0, output->end);
    *output_size = output->tellg();

    return strdup(output->str().c_str());
}

extern "C" void cg3_mwesplit_delete(cg3_mwesplitapplicator *ptr)
{
    cg3_mwesplitapplicator_free(ptr);
}
