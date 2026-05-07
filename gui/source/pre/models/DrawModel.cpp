#include "DrawModel.hpp"
#include "solver/BowModel.hpp"

DrawModel::DrawModel(Draw& draw) {
    BRACE_HEIGHT = addDouble(draw.brace_height);
    DRAW_LENGTH  = addCustom(draw.draw_length);
    NOCK_OFFSET  = addDouble(draw.nock_offset);
}
