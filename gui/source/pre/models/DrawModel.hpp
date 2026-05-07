#pragma once
#include "pre/models/PropertyListModel.hpp"

struct Draw;

class DrawModel: public PropertyListModel {
public:
    QPersistentModelIndex BRACE_HEIGHT;
    QPersistentModelIndex DRAW_LENGTH;
    QPersistentModelIndex NOCK_OFFSET;

    DrawModel(Draw& draw);
};
